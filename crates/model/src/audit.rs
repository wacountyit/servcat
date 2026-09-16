use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::{Id, Timestamp};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct AuditLogEntry {
    pub id: Id,
    pub actor_user_id: Option<Id>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub metadata_json: Option<JsonValue>,
    pub created_at: Timestamp,
}

#[derive(Debug, Clone)]
pub struct NewAuditLogEntry {
    pub actor_user_id: Option<Id>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<String>,
    pub metadata_json: Option<JsonValue>,
}
