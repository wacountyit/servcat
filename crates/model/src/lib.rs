//! Shared domain types for ServCat, used by the db, workflow-engine,
//! connectors, approvals and server crates. Keeping these in one place means
//! the on-disk (JSON/SQL) shape and the in-memory shape never drift apart.

mod approval;
mod audit;
mod catalog;
mod department;
mod instance;
mod org_settings;
mod sql_enum;
mod ticket;
mod user;
mod workflow;

pub use approval::*;
pub use audit::*;
pub use catalog::*;
pub use department::*;
pub use instance::*;
pub use org_settings::*;
pub use ticket::*;
pub use user::*;
pub use workflow::*;

pub type Id = uuid::Uuid;
pub type Timestamp = chrono::DateTime<chrono::Utc>;
