use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::esi::auth::{token_for_character, Character, EsiAuthConfig, EsiToken};

pub const LOCATION_SCOPE: &str = "esi-location.read_location.v1";
const ESI_BASE_URL: &str = "https://esi.evetech.net/latest";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CharacterLocation {
    pub solar_system_id: i32,
}

#[derive(Clone, Debug)]
pub struct LocationClient {
    http: reqwest::Client,
    base_url: String,
}

impl Default for LocationClient {
    fn default() -> Self {
        Self::new()
    }
}

impl LocationClient {
    pub fn new() -> Self {
        Self::with_base_url(ESI_BASE_URL)
    }

    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn get_character_location(
        &self,
        character_id: i64,
        access_token: &str,
    ) -> Result<CharacterLocation> {
        let url = format!("{}/characters/{character_id}/location/", self.base_url);
        let response = self
            .http
            .get(&url)
            .bearer_auth(access_token)
            .send()
            .await
            .with_context(|| format!("failed to request ESI character location for {url}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LocationHttpError { status, body }.into());
        }
        response
            .json::<CharacterLocation>()
            .await
            .context("failed to decode ESI character location response")
    }
}

#[derive(Debug, Error)]
#[error("ESI character location request failed with HTTP {status}: {body}")]
pub struct LocationHttpError {
    pub status: reqwest::StatusCode,
    pub body: String,
}

pub async fn get_character_location(
    character_id: i64,
    access_token: &str,
) -> Result<CharacterLocation> {
    LocationClient::new()
        .get_character_location(character_id, access_token)
        .await
}

pub async fn token_for_location(config: &EsiAuthConfig, character: &Character) -> Result<EsiToken> {
    let token = token_for_character(config, character).await?;
    validate_location_token(&token)?;
    Ok(token)
}

pub fn validate_location_token(token: &EsiToken) -> Result<()> {
    if !token.has_scope(LOCATION_SCOPE) {
        anyhow::bail!("ESI token is missing required scope {LOCATION_SCOPE}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_location_scope_returns_clear_error() {
        let token = EsiToken {
            access_token: "access".to_string(),
            refresh_token: None,
            expires_at: None,
            character_id: Some(42),
            character_name: Some("Pilot".to_string()),
            scopes: vec!["esi-ui.write_waypoint.v1".to_string()],
        };

        let error = validate_location_token(&token).expect_err("location scope should be required");

        assert!(error.to_string().contains(LOCATION_SCOPE));
    }
}
