use async_trait::async_trait;
use serde_json::Value as JsonValue;
use servcat_model::TargetSystem;

use crate::{ConnectorError, DispatchResult, TicketConnector};

/// Generic REST webhook: POSTs the rendered payload as JSON to a fixed URL
/// with an optional bearer token. Works with almost anything that can accept
/// a webhook (Freshservice, Zendesk, a custom internal queue, ...), which is
/// why it's the connector every other one can fall back to.
///
/// `base_url` is admin-configured, not end-user input, but it still points
/// this server at an arbitrary destination -- if workflow authoring is ever
/// opened up to less-trusted admins, put an egress allowlist in front of
/// this connector to avoid SSRF against internal-only services.
pub struct WebhookConnector {
    client: reqwest::Client,
    url: String,
    bearer_token: Option<String>,
}

impl WebhookConnector {
    pub fn new(url: String, bearer_token: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            url,
            bearer_token,
        }
    }
}

#[async_trait]
impl TicketConnector for WebhookConnector {
    fn target_system(&self) -> TargetSystem {
        TargetSystem::Webhook
    }

    async fn dispatch(&self, payload: &JsonValue) -> Result<DispatchResult, ConnectorError> {
        let mut request = self.client.post(&self.url).json(payload);
        if let Some(token) = &self.bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request.send().await?;
        let status = response.status();
        let body: JsonValue = response.json().await.unwrap_or(JsonValue::Null);

        if !status.is_success() {
            return Err(ConnectorError::RejectedByTarget {
                status: status.as_u16(),
                body: body.to_string(),
            });
        }

        // Best-effort: use whatever identifier the receiving system returned,
        // if any; otherwise this is a fire-and-forget webhook with no ticket
        // identity to track.
        let external_ticket_id = body
            .get("id")
            .and_then(JsonValue::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let external_ticket_url = body
            .get("url")
            .and_then(JsonValue::as_str)
            .map(str::to_string);

        Ok(DispatchResult {
            external_ticket_id,
            external_ticket_url,
        })
    }
}
