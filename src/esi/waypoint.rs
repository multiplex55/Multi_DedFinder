use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use reqwest::StatusCode;
use thiserror::Error;
use tokio::time::sleep;

use crate::config::AppConfig;
use crate::esi::auth::{token_for_character, validate_token, Character, EsiToken, WAYPOINT_SCOPE};
use crate::model::route::{GeneratedRoute, RouteWaypoint};

const ESI_BASE_URL: &str = "https://esi.evetech.net/latest";
const MAX_ATTEMPTS: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WaypointRequest {
    pub destination_id: i32,
    pub clear_other_waypoints: bool,
    pub add_to_beginning: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PushOptions {
    pub dry_run: bool,
    pub yes: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PushReport {
    pub pushed: usize,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct EsiWaypointClient {
    http: reqwest::Client,
    base_url: String,
}

impl Default for EsiWaypointClient {
    fn default() -> Self {
        Self::new()
    }
}

impl EsiWaypointClient {
    pub fn new() -> Self {
        Self::with_base_url(ESI_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }
}

#[async_trait]
pub trait WaypointHttpClient: Send + Sync {
    async fn post_waypoint(&self, access_token: &str, request: WaypointRequest) -> Result<()>;
}

#[async_trait]
impl WaypointHttpClient for EsiWaypointClient {
    async fn post_waypoint(&self, access_token: &str, request: WaypointRequest) -> Result<()> {
        let url = format!("{}/ui/autopilot/waypoint/", self.base_url);
        let response = self
            .http
            .post(&url)
            .bearer_auth(access_token)
            .query(&[
                ("destination_id", request.destination_id.to_string()),
                (
                    "clear_other_waypoints",
                    request.clear_other_waypoints.to_string(),
                ),
                ("add_to_beginning", request.add_to_beginning.to_string()),
            ])
            .send()
            .await
            .with_context(|| format!("failed to request ESI waypoint push for {url}"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let retry_after = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        let body = response.text().await.unwrap_or_default();
        Err(WaypointHttpError::new(status, retry_after, body).into())
    }
}

#[derive(Debug, Error)]
#[error("ESI waypoint request failed with HTTP {status}: {body}")]
pub struct WaypointHttpError {
    pub status: StatusCode,
    pub retry_after: Option<u64>,
    pub body: String,
}

impl WaypointHttpError {
    pub fn new(status: StatusCode, retry_after: Option<u64>, body: String) -> Self {
        Self {
            status,
            retry_after,
            body,
        }
    }

    fn is_retryable(&self) -> bool {
        self.status == StatusCode::TOO_MANY_REQUESTS
            || self.status == StatusCode::BAD_GATEWAY
            || self.status == StatusCode::SERVICE_UNAVAILABLE
            || self.status == StatusCode::GATEWAY_TIMEOUT
    }
}

#[derive(Debug, Error)]
#[error(
    "failed to push waypoint #{failed_order} ({failed_system_name}, system ID {failed_system_id}) after pushing {pushed_count} waypoint(s): {source}"
)]
pub struct PartialWaypointPushError {
    pub failed_order: usize,
    pub failed_system_name: String,
    pub failed_system_id: i32,
    pub pushed_count: usize,
    #[source]
    pub source: anyhow::Error,
}

/// Push a generated route to the authenticated character using the official ESI waypoint endpoint.
pub async fn push_waypoints(character: Character, route: &GeneratedRoute) -> Result<PushReport> {
    let config = AppConfig::default();
    let client_id = config
        .esi
        .client_id
        .context("[esi].client_id is required for waypoint push")?;
    let auth_config = crate::esi::auth::EsiAuthConfig::new(
        client_id,
        config.esi.callback_url,
        config.esi.scopes,
    )?;
    let token = token_for_character(&auth_config, &character).await?;
    push_waypoints_with_client(
        &EsiWaypointClient::new(),
        &token,
        &character,
        route,
        &PushOptions::default(),
    )
    .await
}

pub async fn push_waypoints_from_config(
    config: &AppConfig,
    character: Character,
    route: &GeneratedRoute,
    options: &PushOptions,
) -> Result<PushReport> {
    let client_id = config
        .esi
        .client_id
        .clone()
        .context("[esi].client_id is required for waypoint push")?;
    let auth_config = crate::esi::auth::EsiAuthConfig::new(
        client_id,
        config.esi.callback_url.clone(),
        config.esi.scopes.clone(),
    )?;
    let token = token_for_character(&auth_config, &character).await?;
    push_waypoints_with_client(
        &EsiWaypointClient::new(),
        &token,
        &character,
        route,
        options,
    )
    .await
}

pub async fn push_waypoints_with_client<C: WaypointHttpClient>(
    client: &C,
    token: &EsiToken,
    character: &Character,
    route: &GeneratedRoute,
    options: &PushOptions,
) -> Result<PushReport> {
    validate_push_inputs(token, character, route)?;
    if options.dry_run {
        for request in waypoint_requests(&route.waypoints) {
            tracing::info!(
                destination_id = request.destination_id,
                clear_other_waypoints = request.clear_other_waypoints,
                add_to_beginning = request.add_to_beginning,
                "dry-run waypoint push"
            );
        }
        return Ok(PushReport {
            pushed: 0,
            dry_run: true,
        });
    }

    if !options.yes {
        confirm_clear_route()?;
    }

    let requests = waypoint_requests(&route.waypoints);
    let mut pushed = 0;
    for (index, request) in requests.into_iter().enumerate() {
        let waypoint = &route.waypoints[index];
        if let Err(source) = post_with_retries(client, token, request).await {
            return Err(PartialWaypointPushError {
                failed_order: waypoint.order,
                failed_system_name: waypoint.system_name.clone(),
                failed_system_id: waypoint.system_id,
                pushed_count: pushed,
                source,
            }
            .into());
        }
        pushed += 1;
    }

    Ok(PushReport {
        pushed,
        dry_run: false,
    })
}

fn validate_push_inputs(
    token: &EsiToken,
    character: &Character,
    route: &GeneratedRoute,
) -> Result<()> {
    if route.waypoints.is_empty() {
        bail!("cannot push route because it has zero waypoints");
    }
    if !token.has_scope(WAYPOINT_SCOPE) {
        bail!("ESI token is missing required scope {WAYPOINT_SCOPE}");
    }
    validate_token(token, character)?;
    Ok(())
}

fn waypoint_requests(waypoints: &[RouteWaypoint]) -> Vec<WaypointRequest> {
    waypoints
        .iter()
        .enumerate()
        .map(|(index, waypoint)| WaypointRequest {
            destination_id: waypoint.system_id,
            clear_other_waypoints: index == 0,
            add_to_beginning: false,
        })
        .collect()
}

async fn post_with_retries<C: WaypointHttpClient>(
    client: &C,
    token: &EsiToken,
    request: WaypointRequest,
) -> Result<()> {
    let mut last_error = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match client.post_waypoint(&token.access_token, request).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                let retry_delay = retry_delay(&error, attempt);
                last_error = Some(error);
                let Some(delay) = retry_delay else {
                    break;
                };
                tracing::warn!(attempt, ?request, ?delay, "retrying ESI waypoint push");
                sleep(delay).await;
            }
        }
    }
    Err(last_error.context("waypoint push failed without an HTTP response")?)
}

fn retry_delay(error: &anyhow::Error, attempt: usize) -> Option<Duration> {
    if attempt >= MAX_ATTEMPTS {
        return None;
    }
    if let Some(http_error) = error.downcast_ref::<WaypointHttpError>() {
        if !http_error.is_retryable() {
            return None;
        }
        if let Some(seconds) = http_error.retry_after {
            return Some(Duration::from_secs(seconds.min(5)));
        }
        return Some(Duration::from_millis(50 * attempt as u64));
    }
    Some(Duration::from_millis(50 * attempt as u64))
}

fn confirm_clear_route() -> Result<()> {
    use std::io::{self, Write};

    eprint!("This will clear the character's existing in-game route. Type 'yes' to continue: ");
    io::stderr().flush().ok();
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("failed to read waypoint push confirmation")?;
    if answer.trim() != "yes" {
        bail!("waypoint push cancelled before clearing existing route");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use anyhow::anyhow;
    use chrono::TimeZone;
    use reqwest::StatusCode;
    use serde_json::json;

    use super::*;
    use crate::model::route::{GeneratedRoute, RouteLeg, RouteMode, RouteWaypoint};
    use crate::model::score::ScoreBreakdown;

    #[derive(Clone, Default)]
    struct MockWaypointClient {
        requests: Arc<Mutex<Vec<WaypointRequest>>>,
        responses: Arc<Mutex<Vec<Result<(), MockFailure>>>>,
    }

    #[derive(Debug, Clone)]
    enum MockFailure {
        Http(StatusCode),
        RateLimit,
    }

    #[async_trait]
    impl WaypointHttpClient for MockWaypointClient {
        async fn post_waypoint(&self, _access_token: &str, request: WaypointRequest) -> Result<()> {
            self.requests.lock().unwrap().push(request);
            let response = self.responses.lock().unwrap().remove(0);
            match response {
                Ok(()) => Ok(()),
                Err(MockFailure::Http(status)) => {
                    Err(
                        WaypointHttpError::new(status, None, "mock transient failure".to_string())
                            .into(),
                    )
                }
                Err(MockFailure::RateLimit) => Err(WaypointHttpError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    Some(0),
                    "mock rate limit".to_string(),
                )
                .into()),
            }
        }
    }

    impl MockWaypointClient {
        fn with_responses(responses: Vec<Result<(), MockFailure>>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(responses)),
            }
        }

        fn requests(&self) -> Vec<WaypointRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn token() -> EsiToken {
        EsiToken {
            access_token: "access".to_string(),
            refresh_token: Some("refresh".to_string()),
            expires_at: None,
            character_id: Some(42),
            character_name: Some("Pilot".to_string()),
            scopes: vec![WAYPOINT_SCOPE.to_string()],
        }
    }

    fn character() -> Character {
        Character::new(42, Some("Pilot".to_string()))
    }

    fn score_breakdown() -> ScoreBreakdown {
        ScoreBreakdown {
            activity: 0.0,
            distance: 0.0,
            security: 0.0,
            jump_score: 0.0,
            npc_score: 0.0,
            danger_score: 0.0,
            cluster_density_score: 0.0,
            hub_distance_score: 0.0,
            dead_end_penalty: 0.0,
            reuse_penalty: 0.0,
            faction_space_bonus: 0.0,
            total: 0.0,
        }
    }

    fn route(system_ids: &[i32]) -> GeneratedRoute {
        GeneratedRoute {
            start_system: "Jita".to_string(),
            start_system_id: 30_000_142,
            mode: RouteMode::DenseQuiet,
            highsec_only: true,
            total_jumps: 0,
            average_score: 0.0,
            activity_timestamp: chrono::Utc.timestamp_opt(0, 0).unwrap(),
            config_used: json!({}),
            waypoints: system_ids
                .iter()
                .enumerate()
                .map(|(index, system_id)| RouteWaypoint {
                    order: index + 1,
                    system_id: *system_id,
                    system_name: format!("System {system_id}"),
                    security_status: 1.0,
                    region_id: 1,
                    constellation_id: 2,
                    score: 0.0,
                    jumps_last_hour: 0,
                    npc_kills_last_hour: 0,
                    ship_kills_last_hour: 0,
                    pod_kills_last_hour: 0,
                    distance_from_start: 0,
                    score_breakdown: score_breakdown(),
                })
                .collect(),
            legs: Vec::<RouteLeg>::new(),
        }
    }

    async fn push_with_mock(
        mock: &MockWaypointClient,
        token: &EsiToken,
        route: &GeneratedRoute,
    ) -> Result<PushReport> {
        push_waypoints_with_client(
            mock,
            token,
            &character(),
            route,
            &PushOptions {
                dry_run: false,
                yes: true,
            },
        )
        .await
    }

    #[tokio::test]
    async fn first_waypoint_clears_route() {
        let mock = MockWaypointClient::with_responses(vec![Ok(())]);

        push_with_mock(&mock, &token(), &route(&[30000142]))
            .await
            .unwrap();

        assert_eq!(
            mock.requests(),
            vec![WaypointRequest {
                destination_id: 30000142,
                clear_other_waypoints: true,
                add_to_beginning: false,
            }]
        );
    }

    #[tokio::test]
    async fn later_waypoints_append_without_clearing() {
        let mock = MockWaypointClient::with_responses(vec![Ok(()), Ok(()), Ok(())]);

        push_with_mock(&mock, &token(), &route(&[1, 2, 3]))
            .await
            .unwrap();

        assert_eq!(
            mock.requests(),
            vec![
                WaypointRequest {
                    destination_id: 1,
                    clear_other_waypoints: true,
                    add_to_beginning: false,
                },
                WaypointRequest {
                    destination_id: 2,
                    clear_other_waypoints: false,
                    add_to_beginning: false,
                },
                WaypointRequest {
                    destination_id: 3,
                    clear_other_waypoints: false,
                    add_to_beginning: false,
                },
            ]
        );
    }

    #[tokio::test]
    async fn zero_waypoint_route_returns_clear_error() {
        let mock = MockWaypointClient::with_responses(vec![]);
        let error = push_with_mock(&mock, &token(), &route(&[]))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("zero waypoints"));
        assert!(mock.requests().is_empty());
    }

    #[tokio::test]
    async fn missing_scope_returns_clear_error() {
        let mock = MockWaypointClient::with_responses(vec![]);
        let mut token = token();
        token.scopes.clear();

        let error = push_with_mock(&mock, &token, &route(&[1]))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains(WAYPOINT_SCOPE));
        assert!(mock.requests().is_empty());
    }

    #[tokio::test]
    async fn partial_failure_reports_correct_waypoint_and_pushed_count() {
        let mock = MockWaypointClient::with_responses(vec![
            Ok(()),
            Err(MockFailure::Http(StatusCode::BAD_REQUEST)),
        ]);

        let error = push_with_mock(&mock, &token(), &route(&[10, 20, 30]))
            .await
            .unwrap_err();
        let partial = error.downcast_ref::<PartialWaypointPushError>().unwrap();

        assert_eq!(partial.failed_order, 2);
        assert_eq!(partial.failed_system_name, "System 20");
        assert_eq!(partial.failed_system_id, 20);
        assert_eq!(partial.pushed_count, 1);
    }

    #[tokio::test]
    async fn retry_behavior_for_transient_http_errors() {
        let mock = MockWaypointClient::with_responses(vec![
            Err(MockFailure::Http(StatusCode::BAD_GATEWAY)),
            Ok(()),
        ]);

        let report = push_with_mock(&mock, &token(), &route(&[10]))
            .await
            .unwrap();

        assert_eq!(report.pushed, 1);
        assert_eq!(mock.requests().len(), 2);
    }

    #[tokio::test]
    async fn rate_limit_handling_does_not_silently_skip_waypoints() {
        let mock = MockWaypointClient::with_responses(vec![Err(MockFailure::RateLimit); 3]);

        let error = push_with_mock(&mock, &token(), &route(&[10]))
            .await
            .unwrap_err();
        let partial = error.downcast_ref::<PartialWaypointPushError>().unwrap();

        assert_eq!(partial.failed_order, 1);
        assert_eq!(partial.failed_system_id, 10);
        assert_eq!(partial.pushed_count, 0);
        assert_eq!(mock.requests().len(), 3);
        assert!(error.to_string().contains("pushing 0 waypoint"));
    }

    #[tokio::test]
    async fn character_mismatch_returns_clear_error() {
        let mock = MockWaypointClient::with_responses(vec![]);
        let mut token = token();
        token.character_id = Some(7);

        let error = push_with_mock(&mock, &token, &route(&[1]))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("character mismatch"));
        assert!(mock.requests().is_empty());
    }

    #[tokio::test]
    async fn dry_run_does_not_call_client() {
        let mock = MockWaypointClient::with_responses(vec![]);
        let report = push_waypoints_with_client(
            &mock,
            &token(),
            &character(),
            &route(&[1, 2]),
            &PushOptions {
                dry_run: true,
                yes: false,
            },
        )
        .await
        .unwrap();

        assert!(report.dry_run);
        assert_eq!(report.pushed, 0);
        assert!(mock.requests().is_empty());
    }

    #[test]
    fn non_retryable_errors_are_not_retried() {
        let error: anyhow::Error = anyhow!(WaypointHttpError::new(
            StatusCode::BAD_REQUEST,
            None,
            "bad".to_string(),
        ));

        assert_eq!(retry_delay(&error, 1), None);
    }
}
