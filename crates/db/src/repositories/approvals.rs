use chrono::{DateTime, Utc};
use servcat_model::{ApprovalStatus, PendingApproval};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, workflow_instance_id, step_id, approver_user_id, status, comment, \
     decided_at, expires_at, created_at FROM pending_approvals";

pub async fn create(
    pool: &Pool,
    workflow_instance_id: Uuid,
    step_id: &str,
    approver_user_id: Uuid,
    expires_at: Option<DateTime<Utc>>,
) -> Result<PendingApproval, DbError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO pending_approvals \
         (id, workflow_instance_id, step_id, approver_user_id, expires_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(workflow_instance_id)
    .bind(step_id)
    .bind(approver_user_id)
    .bind(expires_at)
    .execute(pool)
    .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "pending_approval", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<PendingApproval>, DbError> {
    let approval = sqlx::query_as::<_, PendingApproval>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(approval)
}

pub async fn list_pending_for_approver(pool: &Pool, approver_user_id: Uuid) -> Result<Vec<PendingApproval>, DbError> {
    let approvals = sqlx::query_as::<_, PendingApproval>(&format!(
        "{SELECT} WHERE approver_user_id = ? AND status = 'pending' ORDER BY created_at"
    ))
    .bind(approver_user_id)
    .fetch_all(pool)
    .await?;
    Ok(approvals)
}

/// Approvals whose `expires_at` has passed and are still `pending` -- polled
/// by the approvals crate's background task to auto-expire and resume the
/// owning instance down its `on_reject` path.
pub async fn list_expired(pool: &Pool) -> Result<Vec<PendingApproval>, DbError> {
    let approvals = sqlx::query_as::<_, PendingApproval>(&format!(
        "{SELECT} WHERE status = 'pending' AND expires_at IS NOT NULL AND expires_at <= CURRENT_TIMESTAMP"
    ))
    .fetch_all(pool)
    .await?;
    Ok(approvals)
}

/// Flips status from `pending` to `status`, atomically via the `WHERE`
/// clause, so a decision and an auto-expiry racing each other can't both
/// apply -- only the first write wins and the second affects zero rows.
pub async fn decide(
    pool: &Pool,
    id: Uuid,
    status: ApprovalStatus,
    comment: Option<&str>,
) -> Result<bool, DbError> {
    let result = sqlx::query(
        "UPDATE pending_approvals SET status = ?, comment = ?, decided_at = CURRENT_TIMESTAMP \
         WHERE id = ? AND status = 'pending'",
    )
    .bind(status)
    .bind(comment)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() > 0)
}
