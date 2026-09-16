mod error;
mod notifier;
mod poller;
mod resolution;
mod service;

pub use error::ApprovalsError;
pub use notifier::{ApprovalNotifier, LoggingNotifier};
pub use poller::{spawn_expiry_poller, ExpiredApprovalHandler};
pub use resolution::resolve_approver_user_id;
pub use service::{create_for_instance, decide, default_notifier};
