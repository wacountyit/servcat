use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use servcat_db::{repositories::approvals as approvals_repo, repositories::users, Pool};
use servcat_model::{ApprovalDecision, ApprovalStatus, ApproverResolution, PendingApproval, User};
use uuid::Uuid;

use crate::{resolution::resolve_approver_user_id, ApprovalNotifier, ApprovalsError};

/// Resolves `resolution` against `requester`, creates the `PendingApproval`
/// row, and fires a (best-effort) notification. Does not touch the owning
/// `WorkflowInstance` row -- the caller (the server's workflow orchestrator)
/// is responsible for setting its status to `awaiting_approval` in the same
/// unit of work that got it here, since it also owns `current_step_id`.
pub async fn create_for_instance(
    pool: &Pool,
    notifier: &dyn ApprovalNotifier,
    workflow_instance_id: Uuid,
    step_id: &str,
    requester: &User,
    resolution: &ApproverResolution,
    timeout_seconds: Option<i64>,
) -> Result<PendingApproval, ApprovalsError> {
    let approver_user_id = resolve_approver_user_id(pool, requester, resolution).await?;
    let expires_at = timeout_seconds.map(|secs| Utc::now() + ChronoDuration::seconds(secs));

    let approval = approvals_repo::create(pool, workflow_instance_id, step_id, approver_user_id, expires_at).await?;

    if let Some(approver) = users::get_by_id(pool, approver_user_id).await? {
        notifier.notify_new_approval(&approval, &approver).await;
    }

    Ok(approval)
}

/// Records an approver's decision. Returns the decided `PendingApproval`;
/// the caller still owns resuming the workflow instance from `on_approve`/
/// `on_reject` (via `workflow-engine::resume_after_approval`) since that may
/// itself require the workflow graph and a connector dispatch.
pub async fn decide(
    pool: &Pool,
    approval_id: Uuid,
    deciding_user_id: Uuid,
    decision: ApprovalDecision,
    comment: Option<String>,
) -> Result<PendingApproval, ApprovalsError> {
    let approval = approvals_repo::get_by_id(pool, approval_id)
        .await?
        .ok_or(ApprovalsError::NotFound(approval_id))?;

    if approval.approver_user_id != deciding_user_id {
        return Err(ApprovalsError::NotTheApprover { approval_id, user_id: deciding_user_id });
    }
    if approval.status != ApprovalStatus::Pending {
        return Err(ApprovalsError::AlreadyDecided(approval_id));
    }

    let status = match decision {
        ApprovalDecision::Approve => ApprovalStatus::Approved,
        ApprovalDecision::Reject => ApprovalStatus::Rejected,
    };

    let flipped = approvals_repo::decide(pool, approval_id, status, comment.as_deref()).await?;
    if !flipped {
        // Lost a race against the expiry poller or a duplicate request.
        return Err(ApprovalsError::AlreadyDecided(approval_id));
    }

    approvals_repo::get_by_id(pool, approval_id)
        .await?
        .ok_or(ApprovalsError::NotFound(approval_id))
}

pub fn default_notifier() -> Arc<dyn ApprovalNotifier> {
    Arc::new(crate::notifier::LoggingNotifier)
}
