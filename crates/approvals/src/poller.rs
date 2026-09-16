use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use servcat_db::{repositories::approvals as approvals_repo, Pool};
use servcat_model::{ApprovalStatus, PendingApproval};

/// Called once per approval that just auto-expired. The handler is
/// responsible for resuming the owning workflow instance down its
/// `on_reject` path -- that requires the workflow graph and (potentially) a
/// connector dispatch, both of which live above this crate, so this trait is
/// how the poller hands control back to the server without depending on it.
#[async_trait]
pub trait ExpiredApprovalHandler: Send + Sync {
    async fn handle_expired(&self, approval: PendingApproval);
}

/// Polls `pending_approvals` for rows whose `expires_at` has passed, flips
/// them to `expired`, and invokes `handler` for each. This is the "durable
/// waiting state" mechanism described for `WaitForApproval`'s optional
/// timeout: no Postgres LISTEN/NOTIFY, no external scheduler, just a cheap
/// interval poll appropriate for this scale.
pub fn spawn_expiry_poller(
    pool: Pool,
    handler: Arc<dyn ExpiredApprovalHandler>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            if let Err(err) = poll_once(&pool, handler.as_ref()).await {
                tracing::error!(error = %err, "approval expiry poll failed");
            }
        }
    })
}

async fn poll_once(pool: &Pool, handler: &dyn ExpiredApprovalHandler) -> Result<(), servcat_db::DbError> {
    let expired = approvals_repo::list_expired(pool).await?;
    for approval in expired {
        let flipped = approvals_repo::decide(
            pool,
            approval.id,
            ApprovalStatus::Expired,
            Some("auto-expired: approval timeout reached"),
        )
        .await?;

        // `decide` only affects a row still `pending`; if a human approver
        // raced the poller and decided it first, `flipped` is false and we
        // must not also resume the instance down the timeout path.
        if flipped {
            handler.handle_expired(approval).await;
        }
    }
    Ok(())
}
