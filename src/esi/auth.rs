use std::fs;
use std::io::{self, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use url::Url;

pub const WAYPOINT_SCOPE: &str = "esi-ui.write_waypoint.v1";
const AUTH_URL: &str = "https://login.eveonline.com/v2/oauth/authorize";
const TOKEN_URL: &str = "https://login.eveonline.com/v2/oauth/token";
const DEFAULT_CALLBACK_URL: &str = "http://localhost:53682/callback";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EsiToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub character_id: Option<i64>,
    #[serde(default)]
    pub character_name: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl EsiToken {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|candidate| candidate == scope)
    }

    pub fn is_expired_or_near_expiry(&self) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        unix_now() + 60 >= expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Character {
    pub id: i64,
    pub name: Option<String>,
}

impl Character {
    pub fn new(id: i64, name: Option<String>) -> Self {
        Self { id, name }
    }
}

#[derive(Clone, Debug)]
pub struct EsiAuthConfig {
    pub client_id: String,
    pub callback_url: String,
    pub scopes: Vec<String>,
    pub token_path: PathBuf,
}

impl EsiAuthConfig {
    pub fn new(
        client_id: impl Into<String>,
        callback_url: Option<String>,
        scopes: Vec<String>,
    ) -> Result<Self> {
        let client_id = client_id.into();
        if client_id.trim().is_empty() {
            bail!("[esi].client_id is required for waypoint push authentication");
        }
        let callback_url = callback_url.unwrap_or_else(|| DEFAULT_CALLBACK_URL.to_string());
        let scopes = ensure_waypoint_scope(scopes);
        Ok(Self {
            client_id,
            callback_url,
            scopes,
            token_path: default_token_path()?,
        })
    }
}

pub fn ensure_waypoint_scope(mut scopes: Vec<String>) -> Vec<String> {
    if !scopes.iter().any(|scope| scope == WAYPOINT_SCOPE) {
        scopes.push(WAYPOINT_SCOPE.to_string());
    }
    scopes
}

pub fn default_token_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not determine local config directory")?;
    Ok(base.join("eve-ded-route").join("esi-token.json"))
}

pub fn load_token(path: &PathBuf) -> Result<Option<EsiToken>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read ESI token from {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse ESI token from {}", path.display()))
        .map(Some)
}

pub fn save_token(path: &PathBuf, token: &EsiToken) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create token directory {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(token).context("failed to serialize ESI token")?;
    write_private(path, contents.as_bytes())
}

#[cfg(unix)]
fn write_private(path: &PathBuf, contents: &[u8]) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("failed to open token file {}", path.display()))?;
    file.write_all(contents)
        .with_context(|| format!("failed to write token file {}", path.display()))
}

#[cfg(not(unix))]
fn write_private(path: &PathBuf, contents: &[u8]) -> Result<()> {
    fs::write(path, contents)
        .with_context(|| format!("failed to write token file {}", path.display()))
}

pub async fn token_for_character(
    config: &EsiAuthConfig,
    character: &Character,
) -> Result<EsiToken> {
    let mut token = match load_token(&config.token_path)? {
        Some(token) => token,
        None => authenticate_interactively(config).await?,
    };

    if token.is_expired_or_near_expiry() {
        token = refresh_token(config, &token).await?;
        save_token(&config.token_path, &token)?;
    }

    validate_token(&token, character)?;
    Ok(token)
}

pub fn validate_token(token: &EsiToken, character: &Character) -> Result<()> {
    if !token.has_scope(WAYPOINT_SCOPE) {
        bail!(
            "ESI token is missing required scope {WAYPOINT_SCOPE}; delete the token file and authenticate again"
        );
    }
    if let Some(token_character_id) = token.character_id {
        if token_character_id != character.id {
            bail!(
                "ESI token character mismatch: token is for character ID {token_character_id}, requested character ID {}",
                character.id
            );
        }
    }
    Ok(())
}

pub async fn authenticate_interactively(config: &EsiAuthConfig) -> Result<EsiToken> {
    let state = format!("{}", unix_now());
    let mut auth_url = Url::parse(AUTH_URL).context("invalid EVE SSO authorization URL")?;
    auth_url
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("redirect_uri", &config.callback_url)
        .append_pair("client_id", &config.client_id)
        .append_pair("scope", &config.scopes.join(" "))
        .append_pair("state", &state);

    eprintln!("Open this EVE SSO URL, authorize the character, then return here:\n{auth_url}");
    let code = if is_localhost_callback(&config.callback_url) {
        eprintln!("Waiting for localhost callback, or paste the code below if the browser cannot reach this CLI.");
        match read_localhost_code(&config.callback_url, &state) {
            Ok(code) => code,
            Err(_) => read_manual_code()?,
        }
    } else {
        read_manual_code()?
    };

    let token = exchange_code(config, &code).await?;
    save_token(&config.token_path, &token)?;
    Ok(token)
}

fn read_manual_code() -> Result<String> {
    eprint!("Authorization code: ");
    io::stderr().flush().ok();
    let mut code = String::new();
    io::stdin()
        .read_line(&mut code)
        .context("failed to read authorization code from stdin")?;
    let code = code.trim().to_string();
    if code.is_empty() {
        bail!("authorization code cannot be empty");
    }
    Ok(code)
}

fn read_localhost_code(callback_url: &str, expected_state: &str) -> Result<String> {
    let callback = Url::parse(callback_url).context("invalid callback URL")?;
    let port = callback
        .port_or_known_default()
        .context("callback URL needs a port")?;
    let path = callback.path().to_string();
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("failed to listen for OAuth callback on port {port}"))?;
    listener
        .set_nonblocking(false)
        .context("failed to configure callback listener")?;
    listener
        .set_ttl(64)
        .context("failed to configure callback listener TTL")?;

    let (mut stream, _) = listener
        .accept()
        .context("failed to receive OAuth callback")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(180)))
        .context("failed to configure callback stream timeout")?;
    let mut buffer = [0_u8; 4096];
    let len = std::io::Read::read(&mut stream, &mut buffer).context("failed to read callback")?;
    let request = String::from_utf8_lossy(&buffer[..len]);
    let first_line = request.lines().next().context("empty callback request")?;
    let target = first_line
        .split_whitespace()
        .nth(1)
        .context("malformed callback request")?;
    let parsed = Url::parse(&format!("http://localhost{target}"))?;
    if parsed.path() != path {
        bail!("unexpected callback path {}", parsed.path());
    }
    let code = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .context("callback did not include an authorization code")?;
    let state = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
        .context("callback did not include OAuth state")?;
    if state != expected_state {
        bail!("OAuth callback state did not match the login request");
    }
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nAuthorization received. You can return to eve-ded-route.\n";
    let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
    Ok(code)
}

fn is_localhost_callback(callback_url: &str) -> bool {
    Url::parse(callback_url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host == "localhost" || host == "127.0.0.1")
        })
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

async fn exchange_code(config: &EsiAuthConfig, code: &str) -> Result<EsiToken> {
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", &config.client_id),
        ("redirect_uri", &config.callback_url),
    ];
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("failed to exchange EVE SSO authorization code")?
        .error_for_status()
        .context("EVE SSO authorization code exchange failed")?
        .json::<TokenResponse>()
        .await
        .context("failed to decode EVE SSO token response")?;
    Ok(token_from_response(response))
}

pub async fn refresh_token(config: &EsiAuthConfig, token: &EsiToken) -> Result<EsiToken> {
    let refresh_token = token
        .refresh_token
        .as_deref()
        .context("ESI token expired and no refresh token is available; authenticate again")?;
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", &config.client_id),
    ];
    let response = reqwest::Client::new()
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("failed to refresh EVE SSO token")?
        .error_for_status()
        .context("EVE SSO token refresh failed")?
        .json::<TokenResponse>()
        .await
        .context("failed to decode refreshed EVE SSO token")?;
    let mut refreshed = token_from_response(response);
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = token.refresh_token.clone();
    }
    Ok(refreshed)
}

fn token_from_response(response: TokenResponse) -> EsiToken {
    let mut token = EsiToken {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        expires_at: response.expires_in.map(|seconds| unix_now() + seconds),
        character_id: None,
        character_name: None,
        scopes: Vec::new(),
    };
    apply_jwt_claims(&mut token);
    token
}

pub fn apply_jwt_claims(token: &mut EsiToken) {
    let Some(claims) = decode_jwt_claims(&token.access_token) else {
        return;
    };
    if token.expires_at.is_none() {
        token.expires_at = claims.exp;
    }
    if token.character_id.is_none() {
        token.character_id = claims
            .sub
            .as_deref()
            .and_then(|sub| sub.strip_prefix("CHARACTER:EVE:"))
            .and_then(|id| id.parse().ok());
    }
    if token.character_name.is_none() {
        token.character_name = claims.name;
    }
    if token.scopes.is_empty() {
        token.scopes = claims.scp.unwrap_or_default();
    }
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    exp: Option<i64>,
    #[serde(default)]
    scp: Option<Vec<String>>,
}

fn decode_jwt_claims(access_token: &str) -> Option<JwtClaims> {
    let claims = access_token.split('.').nth(1)?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(claims)
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
