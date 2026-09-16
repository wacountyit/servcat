use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use servcat_model::TargetSystem;

use crate::{ConnectorError, DispatchResult, TicketConnector};

/// GLPI REST API (`apirest.php`), session-token based. `base_url` is the
/// GLPI root (e.g. `https://glpi.example.com`), not the `apirest.php` path
/// itself. `app_token` is the API client's application token (GLPI Setup,
/// General, API); `user_token` is a per-user personal token (User Settings,
/// Remote access keys) used only to establish a session -- GLPI issues a
/// short-lived `Session-Token` for the actual `Ticket` creation call, which
/// this connector closes with `killSession` once dispatch completes.
pub struct GlpiConnector {
    client: reqwest::Client,
    base_url: String,
    app_token: String,
    user_token: String,
}

impl GlpiConnector {
    pub fn new(base_url: String, app_token: String, user_token: String) -> Self {
        Self { client: reqwest::Client::new(), base_url: base_url.trim_end_matches('/').to_string(), app_token, user_token }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/apirest.php/{}", self.base_url, path)
    }

    async fn init_session(&self) -> Result<String, ConnectorError> {
        let response = self
            .client
            .get(self.api_url("initSession"))
            .header("App-Token", &self.app_token)
            .header("Authorization", format!("user_token {}", self.user_token))
            .send()
            .await?;

        let status = response.status();
        let body: JsonValue = response.json().await.unwrap_or(JsonValue::Null);
        if !status.is_success() {
            return Err(ConnectorError::RejectedByTarget { status: status.as_u16(), body: body.to_string() });
        }

        body.get("session_token")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .ok_or_else(|| ConnectorError::UnexpectedResponse("missing 'session_token' in GLPI response".into()))
    }

    async fn kill_session(&self, session_token: &str) {
        let result = self
            .client
            .get(self.api_url("killSession"))
            .header("App-Token", &self.app_token)
            .header("Session-Token", session_token)
            .send()
            .await;
        if let Err(err) = result {
            tracing::warn!(error = %err, "failed to close GLPI session cleanly");
        }
    }
}

#[async_trait]
impl TicketConnector for GlpiConnector {
    fn target_system(&self) -> TargetSystem {
        TargetSystem::Glpi
    }

    async fn dispatch(&self, payload: &JsonValue) -> Result<DispatchResult, ConnectorError> {
        let session_token = self.init_session().await?;

        let result = async {
            let response = self
                .client
                .post(self.api_url("Ticket"))
                .header("App-Token", &self.app_token)
                .header("Session-Token", &session_token)
                .json(&json!({ "input": payload }))
                .send()
                .await?;

            let status = response.status();
            let body: JsonValue = response.json().await.unwrap_or(JsonValue::Null);
            if !status.is_success() {
                return Err(ConnectorError::RejectedByTarget { status: status.as_u16(), body: body.to_string() });
            }

            let id = body
                .get("id")
                .and_then(JsonValue::as_u64)
                .ok_or_else(|| ConnectorError::UnexpectedResponse("missing 'id' in GLPI response".into()))?;

            Ok(DispatchResult {
                external_ticket_id: id.to_string(),
                external_ticket_url: Some(format!("{}/front/ticket.form.php?id={}", self.base_url, id)),
            })
        }
        .await;

        self.kill_session(&session_token).await;
        result
    }
}
