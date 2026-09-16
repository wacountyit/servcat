use servcat_model::{Department, NewDepartment, UpdateDepartment};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, name, logo_url, parent_department_id, created_at, updated_at FROM departments";

pub async fn create(pool: &Pool, new: &NewDepartment) -> Result<Department, DbError> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO departments (id, name, parent_department_id) VALUES (?, ?, ?)")
        .bind(id)
        .bind(&new.name)
        .bind(new.parent_department_id)
        .execute(pool)
        .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "department", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<Department>, DbError> {
    let dept = sqlx::query_as::<_, Department>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(dept)
}

pub async fn list(pool: &Pool) -> Result<Vec<Department>, DbError> {
    let depts = sqlx::query_as::<_, Department>(&format!("{SELECT} ORDER BY name"))
        .fetch_all(pool)
        .await?;
    Ok(depts)
}

/// Note: `COALESCE` means `None` in the patch always leaves a field
/// unchanged; there's currently no way to explicitly clear
/// `parent_department_id` back to a top-level department through this call.
pub async fn update(pool: &Pool, id: Uuid, patch: &UpdateDepartment) -> Result<Option<Department>, DbError> {
    sqlx::query(
        "UPDATE departments SET \
            name = COALESCE(?, name), \
            parent_department_id = COALESCE(?, parent_department_id) \
         WHERE id = ?",
    )
    .bind(&patch.name)
    .bind(patch.parent_department_id)
    .bind(id)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await
}

pub async fn set_logo_url(pool: &Pool, id: Uuid, logo_url: Option<&str>) -> Result<Option<Department>, DbError> {
    sqlx::query("UPDATE departments SET logo_url = ? WHERE id = ?")
        .bind(logo_url)
        .bind(id)
        .execute(pool)
        .await?;

    get_by_id(pool, id).await
}

pub async fn delete(pool: &Pool, id: Uuid) -> Result<(), DbError> {
    sqlx::query("DELETE FROM departments WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
