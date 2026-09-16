pub mod field_mapping;
pub mod glpi;
pub mod jira;
pub mod webhook;

use async_trait::async_trait;
use serde_json::Value as JsonValue;

pub use field_mapping::{TemplateContext, render as render_field_mapping};
pub use glpi::GlpiConnector;
pub use jira::JiraConnector;
pub use webhook::WebhookConnector;

#[derive(Debug, thiserror::Error)]
pub enum ConnectorError {
    #[error("request to target system failed: {0}")]
    Transport(#[from] reqwest::Error),

    #[error("target system rejected the request (HTTP {status}): {body}")]
    RejectedByTarget { status: u16, body: String },

    #[error("target system response could not be parsed: {0}")]
    UnexpectedResponse(String),

    #[error("connector misconfigured: {0}")]
    Config(String),
}

#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub external_ticket_id: String,
    pub external_ticket_url: Option<String>,
}

/// Implemented once per target ticketing system. `payload` is the already
/// rendered/nested JSON object produced by `render_field_mapping` for that
/// workflow's `field_mapping` -- connectors should not need to know about
/// answers, templates, or the workflow graph at all.
#[async_trait]
pub trait TicketConnector: Send + Sync {
    fn target_system(&self) -> servcat_model::TargetSystem;

    async fn dispatch(&self, payload: &JsonValue) -> Result<DispatchResult, ConnectorError>;
}
