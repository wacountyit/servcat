use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{sql_enum::sql_string_enum, Id, Timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceStatus {
    InProgress,
    AwaitingApproval,
    Completed,
    Rejected,
    Cancelled,
    Failed,
}

sql_string_enum!(InstanceStatus {
    InProgress => "in_progress",
    AwaitingApproval => "awaiting_approval",
    Completed => "completed",
    Rejected => "rejected",
    Cancelled => "cancelled",
    Failed => "failed",
});

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkflowInstance {
    pub id: Id,
    pub workflow_definition_id: Id,
    pub catalog_item_id: Id,
    pub requester_user_id: Id,
    pub status: InstanceStatus,
    pub current_step_id: String,
    pub answers_json: JsonValue,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub completed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StartWorkflowInstance {
    pub catalog_item_id: Id,
}

/// One answer submitted for the instance's current `Question` step.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmitAnswer {
    pub field_key: String,
    pub value: JsonValue,
}
