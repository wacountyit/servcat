use async_trait::async_trait;
use servcat_model::{PendingApproval, User};

/// Fired once when a `PendingApproval` is created. The starter implementation
/// only logs -- wire this to real email/Slack/Teams before relying on it in
/// production; nothing in the approval flow depends on notification actually
/// succeeding (a missed notification does not block an approver from acting,
/// since they can still see it in their in-app approvals inbox).
#[async_trait]
pub trait ApprovalNotifier: Send + Sync {
    async fn notify_new_approval(&self, approval: &PendingApproval, approver: &User);
}

pub struct LoggingNotifier;

#[async_trait]
impl ApprovalNotifier for LoggingNotifier {
    async fn notify_new_approval(&self, approval: &PendingApproval, approver: &User) {
        tracing::info!(
            approval_id = %approval.id,
            approver_email = %approver.email,
            step_id = %approval.step_id,
            "TODO: send a real notification -- approval is pending"
        );
    }
}
