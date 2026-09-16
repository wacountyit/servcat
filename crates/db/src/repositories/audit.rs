use servcat_model::NewAuditLogEntry;
use uuid::Uuid;

use crate::{DbError, Pool};

pub async fn record(pool: &Pool, entry: &NewAuditLogEntry) -> Result<(), DbError> {
    sqlx::query(
        "INSERT INTO audit_log (id, actor_user_id, action, entity_type, entity_id, metadata_json) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4())
    .bind(entry.actor_user_id)
    .bind(&entry.action)
    .bind(&entry.entity_type)
    .bind(&entry.entity_id)
    .bind(&entry.metadata_json)
    .execute(pool)
    .await?;
    Ok(())
}
