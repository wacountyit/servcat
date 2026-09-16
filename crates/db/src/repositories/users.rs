use servcat_model::{NewUser, Role, UpdateUser, User};
use uuid::Uuid;

use crate::{DbError, Pool};

const SELECT: &str = "SELECT id, email, display_name, password_hash, external_idp_subject, \
     department_id, manager_user_id, role, is_active, created_at, updated_at FROM users";

/// Inserts a new user. `password_hash` must already be an Argon2id hash
/// (or `None` for an SSO-only account) -- this layer never sees, hashes, or
/// logs a plaintext password.
pub async fn create(pool: &Pool, new: &NewUser, password_hash: Option<&str>) -> Result<User, DbError> {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash, external_idp_subject, \
         department_id, manager_user_id, role) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind(&new.email)
    .bind(&new.display_name)
    .bind(password_hash)
    .bind(&new.external_idp_subject)
    .bind(new.department_id)
    .bind(new.manager_user_id)
    .bind(new.role)
    .execute(pool)
    .await?;

    get_by_id(pool, id)
        .await?
        .ok_or_else(|| DbError::NotFound { entity: "user", id: id.to_string() })
}

pub async fn get_by_id(pool: &Pool, id: Uuid) -> Result<Option<User>, DbError> {
    let user = sqlx::query_as::<_, User>(&format!("{SELECT} WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_by_email(pool: &Pool, email: &str) -> Result<Option<User>, DbError> {
    let user = sqlx::query_as::<_, User>(&format!("{SELECT} WHERE email = ?"))
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_by_external_idp_subject(pool: &Pool, subject: &str) -> Result<Option<User>, DbError> {
    let user = sqlx::query_as::<_, User>(&format!("{SELECT} WHERE external_idp_subject = ?"))
        .bind(subject)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn list(pool: &Pool, include_inactive: bool) -> Result<Vec<User>, DbError> {
    let sql = if include_inactive {
        format!("{SELECT} ORDER BY display_name")
    } else {
        format!("{SELECT} WHERE is_active = TRUE ORDER BY display_name")
    };
    let users = sqlx::query_as::<_, User>(&sql).fetch_all(pool).await?;
    Ok(users)
}

/// Users holding `role`, optionally narrowed to a single department -- used by
/// the approvals crate to resolve `ApproverResolution::RoleInDepartment`.
pub async fn list_active_by_role_in_department(
    pool: &Pool,
    role: Role,
    department_id: Uuid,
) -> Result<Vec<User>, DbError> {
    let users = sqlx::query_as::<_, User>(&format!(
        "{SELECT} WHERE role = ? AND department_id = ? AND is_active = TRUE ORDER BY display_name"
    ))
    .bind(role)
    .bind(department_id)
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// Note: `COALESCE` means `None` in the patch always leaves a field
/// unchanged; there's currently no way to explicitly clear
/// `department_id`/`manager_user_id` through this call.
pub async fn update(pool: &Pool, id: Uuid, patch: &UpdateUser) -> Result<Option<User>, DbError> {
    sqlx::query(
        "UPDATE users SET \
            display_name = COALESCE(?, display_name), \
            department_id = COALESCE(?, department_id), \
            manager_user_id = COALESCE(?, manager_user_id), \
            role = COALESCE(?, role), \
            is_active = COALESCE(?, is_active) \
         WHERE id = ?",
    )
    .bind(&patch.display_name)
    .bind(patch.department_id)
    .bind(patch.manager_user_id)
    .bind(patch.role)
    .bind(patch.is_active)
    .bind(id)
    .execute(pool)
    .await?;

    get_by_id(pool, id).await
}

/// Links a previously admin-provisioned, password-less account (created with
/// just an email, ahead of that person's first SSO login) to the IdP
/// subject asserted on that first login. Only called when the target row's
/// `external_idp_subject` is currently `NULL` -- see `routes/sso.rs`.
pub async fn link_external_idp_subject(pool: &Pool, id: Uuid, subject: &str) -> Result<Option<User>, DbError> {
    sqlx::query("UPDATE users SET external_idp_subject = ? WHERE id = ?")
        .bind(subject)
        .bind(id)
        .execute(pool)
        .await?;

    get_by_id(pool, id).await
}

pub async fn set_password_hash(pool: &Pool, id: Uuid, password_hash: &str) -> Result<(), DbError> {
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(password_hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deactivates rather than deletes: `WorkflowInstance.requester_user_id` and
/// `PendingApproval.approver_user_id` reference users with `ON DELETE
/// RESTRICT`, so a departed employee's history stays intact and auditable.
pub async fn deactivate(pool: &Pool, id: Uuid) -> Result<(), DbError> {
    sqlx::query("UPDATE users SET is_active = FALSE WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
