use servcat_model::{AuditLogEntry, NewAuditLogEntry};
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

/// Newest-first page of the audit trail. `page` is zero-based; `page_size`
/// is the caller's responsibility to cap (see `routes/audit_log.rs` and
/// `web/pages/admin/audit_log.rs`, which both clamp it before calling this).
pub async fn list(pool: &Pool, page: i64, page_size: i64) -> Result<Vec<AuditLogEntry>, DbError> {
    let entries = sqlx::query_as::<_, AuditLogEntry>(
        "SELECT id, actor_user_id, action, entity_type, entity_id, metadata_json, created_at \
         FROM audit_log ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
    )
    .bind(page_size)
    .bind(page.max(0) * page_size)
    .fetch_all(pool)
    .await?;
    Ok(entries)
}

pub async fn count(pool: &Pool) -> Result<i64, DbError> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM audit_log")
        .fetch_one(pool)
        .await?;
    Ok(count)
}
