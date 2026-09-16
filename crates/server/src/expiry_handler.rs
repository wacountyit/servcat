use async_trait::async_trait;
use servcat_approvals::ExpiredApprovalHandler;
use servcat_model::{ApprovalDecision, PendingApproval};

use crate::{orchestrator, state::AppState};

/// Bridges the approvals crate's timeout poller back into the orchestrator:
/// an expired `WaitForApproval` step is treated as a rejection, resuming the
/// instance down its `on_reject` path.
pub struct ServerExpiryHandler(pub AppState);

#[async_trait]
impl ExpiredApprovalHandler for ServerExpiryHandler {
    async fn handle_expired(&self, approval: PendingApproval) {
        let deps = orchestrator::Deps {
            pool: &self.0.pool,
            notifier: self.0.notifier.as_ref(),
            connectors: &self.0.connectors,
        };
        let result = orchestrator::resume_after_approval(
            &deps,
            approval.workflow_instance_id,
            &approval.step_id,
            ApprovalDecision::Reject,
        )
        .await;

        if let Err(err) = result {
            tracing::error!(
                approval_id = %approval.id,
                error = %err,
                "failed to resume workflow instance after approval expiry"
            );
        }
    }
}
