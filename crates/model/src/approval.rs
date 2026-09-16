use serde::{Deserialize, Serialize};

use crate::{Id, Timestamp, sql_enum::sql_string_enum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

sql_string_enum!(ApprovalStatus {
    Pending => "pending",
    Approved => "approved",
    Rejected => "rejected",
    Expired => "expired",
});

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PendingApproval {
    pub id: Id,
    pub workflow_instance_id: Id,
    pub step_id: String,
    pub approver_user_id: Id,
    pub status: ApprovalStatus,
    pub comment: Option<String>,
    pub decided_at: Option<Timestamp>,
    pub expires_at: Option<Timestamp>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DecideApproval {
    pub decision: ApprovalDecision,
    pub comment: Option<String>,
}
