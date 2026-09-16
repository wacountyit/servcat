use serde_json::Value as JsonValue;
use servcat_model::{
    FieldMapping, NewWorkflowDefinition, TargetSystem, WorkflowDefinition, WorkflowGraph,
};
use uuid::Uuid;

use crate::{DbError, Pool};

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    name: String,
    version: i32,
    definition_json: JsonValue,
    field_mapping_json: Option<JsonValue>,
    target_system: Option<TargetSystem>,
    is_published: bool,
    created_by: Option<Uuid>,
    created_at: servcat_model::Timestamp,
    updated_at: servcat_model::Timestamp,
}

impl Row {
    fn into_model(self) -> Result<WorkflowDefinition, DbError> {
        let entity = "workflow_definition";
        let graph: WorkflowGraph =
            serde_json::from_value(self.definition_json).map_err(|source| {
                DbError::CorruptJson {
                    entity,
                    id: self.id.to_string(),
                    source,
                }
            })?;
        let field_mapping = self
            .field_mapping_json
            .map(serde_json::from_value::<FieldMapping>)
            .transpose()
            .map_err(|source| DbError::CorruptJson {
                entity,
                id: self.id.to_string(),
                source,
            })?;

        Ok(WorkflowDefinition {
            id: self.id,
            name: self.name,
            version: self.version,
            graph,
            field_mapping,
            target_system: self.target_system,
            is_published: self.is_published,
            created_by: self.created_by,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const SELECT: &str = "SELECT id, name, version, definition_json, field_mapping_json, target_system, \
     is_published, created_by, created_at, updated_at FROM workflow_definitions";

pub async fn create(
    pool: &Pool,
    new: &NewWorkflowDefinition,
    created_by: Uuid,
) -> Result<WorkflowDefinition, DbError> {
    let id = Uuid::new_v4();
    let definition_json =
        serde_json::to_value(&new.graph).expect("WorkflowGraph always serializes");
    let field_mapping_json = new
        .field_mapping
        .as_ref()
        .map(|m| serde_json::to_value(m).expect("FieldMapping always serializes"));

    sqlx::query(
        "INSERT INTO workflow_definitions \
         (id, name, version, definition_json, field_mapping_json, target_system, created_by) \
         VALUES (?, ?, 1, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&new.name)
    .bind(&definition_json)
    .bind(&field_mapping_json)
    .bind(new.target_system)
    .bind(created_by)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await?.ok_or_else(|| DbError::NotFound {
        entity: "workflow_definition",
        id: id.to_string(),
    })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<WorkflowDefinition>, DbError> {
    let row = sqlx::query_as::<_, Row>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(Row::into_model).transpose()
}

pub async fn list(pool: &Pool, published_only: bool) -> Result<Vec<WorkflowDefinition>, DbError> {
    let sql = if published_only {
        format!("{SELECT} WHERE is_published = TRUE ORDER BY name, version DESC")
    } else {
        format!("{SELECT} ORDER BY name, version DESC")
    };
    let rows = sqlx::query_as::<_, Row>(&sql).fetch_all(pool).await?;
    rows.into_iter().map(Row::into_model).collect()
}

/// Publishing is a separate, explicit action (rather than implied by create)
/// so an admin can author/preview a graph before it becomes selectable by a
/// catalog item.
pub async fn set_published(pool: &Pool, id: Uuid, is_published: bool) -> Result<(), DbError> {
    sqlx::query("UPDATE workflow_definitions SET is_published = ? WHERE id = ?")
        .bind(is_published)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
