use servcat_model::{NewServiceCatalogItem, ServiceCatalogItem, UpdateServiceCatalogItem};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, name, description, category, icon, workflow_definition_id, \
     is_active, created_by, created_at, updated_at FROM service_catalog_items";

pub async fn create(
    pool: &Pool,
    new: &NewServiceCatalogItem,
    created_by: Uuid,
) -> Result<ServiceCatalogItem, DbError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO service_catalog_items \
         (id, name, description, category, icon, workflow_definition_id, created_by) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&new.name)
    .bind(&new.description)
    .bind(&new.category)
    .bind(&new.icon)
    .bind(new.workflow_definition_id)
    .bind(created_by)
    .execute(pool)
    .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "service_catalog_item", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<ServiceCatalogItem>, DbError> {
    let item = sqlx::query_as::<_, ServiceCatalogItem>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(item)
}

/// Items visible on the self-service catalog. Admins pass
/// `include_inactive = true` to manage draft/retired items too.
pub async fn list(pool: &Pool, include_inactive: bool) -> Result<Vec<ServiceCatalogItem>, DbError> {
    let sql = if include_inactive {
        format!("{SELECT} ORDER BY category, name")
    } else {
        format!("{SELECT} WHERE is_active = TRUE ORDER BY category, name")
    };
    let items = sqlx::query_as::<_, ServiceCatalogItem>(&sql).fetch_all(pool).await?;
    Ok(items)
}

pub async fn update(
    pool: &Pool,
    id: Uuid,
    patch: &UpdateServiceCatalogItem,
) -> Result<Option<ServiceCatalogItem>, DbError> {
    sqlx::query(
        "UPDATE service_catalog_items SET \
            name = COALESCE(?, name), \
            description = COALESCE(?, description), \
            category = COALESCE(?, category), \
            icon = COALESCE(?, icon), \
            is_active = COALESCE(?, is_active) \
         WHERE id = ?",
    )
    .bind(&patch.name)
    .bind(&patch.description)
    .bind(&patch.category)
    .bind(&patch.icon)
    .bind(patch.is_active)
    .bind(id)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await
}

/// Retires the catalog item without touching history. Prefer this over a
/// hard delete: `workflow_instances.catalog_item_id` has `ON DELETE
/// RESTRICT`, so a hard delete would fail once anyone has submitted a
/// request against it.
pub async fn deactivate(pool: &Pool, id: Uuid) -> Result<(), DbError> {
    sqlx::query("UPDATE service_catalog_items SET is_active = FALSE WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
