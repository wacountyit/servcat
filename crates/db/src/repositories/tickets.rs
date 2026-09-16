use serde_json::Value as JsonValue;
use servcat_model::{DispatchStatus, TargetSystem, Ticket};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, workflow_instance_id, target_system, rendered_payload_json, \
     external_ticket_id, external_ticket_url, dispatch_status, last_error, created_at, updated_at FROM tickets";

pub async fn create(
    pool: &Pool,
    workflow_instance_id: Uuid,
    target_system: TargetSystem,
    rendered_payload_json: &JsonValue,
) -> Result<Ticket, DbError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO tickets (id, workflow_instance_id, target_system, rendered_payload_json) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(workflow_instance_id)
    .bind(target_system)
    .bind(rendered_payload_json)
    .execute(pool)
    .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "ticket", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<Ticket>, DbError> {
    let ticket = sqlx::query_as::<_, Ticket>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(ticket)
}

pub async fn list_pending_dispatch(pool: &Pool) -> Result<Vec<Ticket>, DbError> {
    let tickets = sqlx::query_as::<_, Ticket>(&format!(
        "{SELECT} WHERE dispatch_status = 'pending' ORDER BY created_at"
    ))
    .fetch_all(pool)
    .await?;
    Ok(tickets)
}

pub async fn mark_sent(
    pool: &Pool,
    id: Uuid,
    external_ticket_id: &str,
    external_ticket_url: Option<&str>,
) -> Result<(), DbError> {
    sqlx::query(
        "UPDATE tickets SET dispatch_status = 'sent', external_ticket_id = ?, \
         external_ticket_url = ?, last_error = NULL WHERE id = ?",
    )
    .bind(external_ticket_id)
    .bind(external_ticket_url)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_failed(pool: &Pool, id: Uuid, error: &str) -> Result<(), DbError> {
    sqlx::query("UPDATE tickets SET dispatch_status = 'failed', last_error = ? WHERE id = ?")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_dispatch_status(pool: &Pool, id: Uuid, status: DispatchStatus) -> Result<(), DbError> {
    sqlx::query("UPDATE tickets SET dispatch_status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
