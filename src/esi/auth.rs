use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct EsiToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
}
