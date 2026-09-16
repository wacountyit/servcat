use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{sql_enum::sql_string_enum, Id, TargetSystem, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    Pending,
    Sent,
    Failed,
    Acked,
}

sql_string_enum!(DispatchStatus {
    Pending => "pending",
    Sent => "sent",
    Failed => "failed",
    Acked => "acked",
});

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Ticket {
    pub id: Id,
    pub workflow_instance_id: Id,
    pub target_system: TargetSystem,
    pub rendered_payload_json: JsonValue,
    pub external_ticket_id: Option<String>,
    pub external_ticket_url: Option<String>,
    pub dispatch_status: DispatchStatus,
    pub last_error: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
