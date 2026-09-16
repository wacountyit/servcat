use async_trait::async_trait;
use serde_json::{Value as JsonValue, json};
use servcat_model::TargetSystem;

use crate::{ConnectorError, DispatchResult, TicketConnector};

/// Jira Cloud/Server REST API (`/rest/api/3/issue`, API-token basic auth).
/// The rendered `field_mapping` payload is expected to already be shaped
/// like Jira's `fields` object (e.g. target fields `"project.key"`,
/// `"issuetype.name"`, `"summary"`, `"priority.name"`), since Jira's project
/// scheme, issue types and required custom fields vary per org/project and
/// are exactly what an admin configures via that mapping rather than code.
pub struct JiraConnector {
    client: reqwest::Client,
    base_url: String,
    email: String,
    api_token: String,
}

impl JiraConnector {
    pub fn new(base_url: String, email: String, api_token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            email,
            api_token,
        }
    }
}

#[async_trait]
impl TicketConnector for JiraConnector {
    fn target_system(&self) -> TargetSystem {
        TargetSystem::Jira
    }

    async fn dispatch(&self, payload: &JsonValue) -> Result<DispatchResult, ConnectorError> {
        let url = format!("{}/rest/api/3/issue", self.base_url);
        let body = json!({ "fields": payload });

        let response = self
            .client
            .post(&url)
            .basic_auth(&self.email, Some(&self.api_token))
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let body: JsonValue = response.json().await.unwrap_or(JsonValue::Null);

        if !status.is_success() {
            return Err(ConnectorError::RejectedByTarget {
                status: status.as_u16(),
                body: body.to_string(),
            });
        }

        let key = body.get("key").and_then(JsonValue::as_str).ok_or_else(|| {
            ConnectorError::UnexpectedResponse("missing 'key' in Jira response".into())
        })?;

        Ok(DispatchResult {
            external_ticket_id: key.to_string(),
            external_ticket_url: Some(format!("{}/browse/{}", self.base_url, key)),
        })
    }
}
