use serde_json::Value as JsonValue;
use servcat_model::{InstanceStatus, WorkflowInstance};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, workflow_definition_id, catalog_item_id, requester_user_id, status, \
     current_step_id, answers_json, created_at, updated_at, completed_at FROM workflow_instances";

pub async fn create(
    pool: &Pool,
    workflow_definition_id: Uuid,
    catalog_item_id: Uuid,
    requester_user_id: Uuid,
    entry_step_id: &str,
) -> Result<WorkflowInstance, DbError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO workflow_instances \
         (id, workflow_definition_id, catalog_item_id, requester_user_id, current_step_id, answers_json) \
         VALUES (?, ?, ?, ?, ?, JSON_OBJECT())",
    )
    .bind(id)
    .bind(workflow_definition_id)
    .bind(catalog_item_id)
    .bind(requester_user_id)
    .bind(entry_step_id)
    .execute(pool)
    .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "workflow_instance", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<WorkflowInstance>, DbError> {
    let instance = sqlx::query_as::<_, WorkflowInstance>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(instance)
}

pub async fn list_for_requester(pool: &Pool, requester_user_id: Uuid) -> Result<Vec<WorkflowInstance>, DbError> {
    let instances = sqlx::query_as::<_, WorkflowInstance>(&format!(
        "{SELECT} WHERE requester_user_id = ? ORDER BY created_at DESC"
    ))
    .bind(requester_user_id)
    .fetch_all(pool)
    .await?;
    Ok(instances)
}

pub async fn list_awaiting_approval(pool: &Pool) -> Result<Vec<WorkflowInstance>, DbError> {
    let instances = sqlx::query_as::<_, WorkflowInstance>(&format!(
        "{SELECT} WHERE status = 'awaiting_approval' ORDER BY created_at"
    ))
    .fetch_all(pool)
    .await?;
    Ok(instances)
}

/// Persists the workflow-engine's result of advancing an instance: its new
/// current step, merged answers, and status. Terminal statuses also stamp
/// `completed_at`.
pub async fn save_progress(
    pool: &Pool,
    id: Uuid,
    current_step_id: &str,
    answers_json: &JsonValue,
    status: InstanceStatus,
) -> Result<Option<WorkflowInstance>, DbError> {
    let is_terminal = matches!(
        status,
        InstanceStatus::Completed | InstanceStatus::Rejected | InstanceStatus::Cancelled | InstanceStatus::Failed
    );

    sqlx::query(
        "UPDATE workflow_instances SET \
            current_step_id = ?, \
            answers_json = ?, \
            status = ?, \
            completed_at = CASE WHEN ? THEN CURRENT_TIMESTAMP ELSE completed_at END \
         WHERE id = ?",
    )
    .bind(current_step_id)
    .bind(answers_json)
    .bind(status)
    .bind(is_terminal)
    .bind(id)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await
}
