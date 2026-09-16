use servcat_model::{OrgSettings, UpdateOrgSettings};

use crate::{DbError, Pool};

const SELECT: &str = "SELECT app_name, logo_url, allow_local_signup, updated_at FROM org_settings WHERE id = 1";

/// Inserts the single settings row the first time the server starts against
/// a fresh database (no-op once it exists) -- same bootstrap-once pattern as
/// the admin account in `main.rs`. `app_name` is only applied on that first
/// insert; use `update` afterwards to change it at runtime.
pub async fn seed_default(pool: &Pool, app_name: Option<&str>, allow_local_signup: bool) -> Result<(), DbError> {
    sqlx::query("INSERT IGNORE INTO org_settings (id, app_name, allow_local_signup) VALUES (1, ?, ?)")
        .bind(app_name.unwrap_or("ServCat"))
        .bind(allow_local_signup)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get(pool: &Pool) -> Result<OrgSettings, DbError> {
    let settings = sqlx::query_as::<_, OrgSettings>(SELECT).fetch_one(pool).await?;
    Ok(settings)
}

/// Note: `COALESCE` means `None` in the patch always leaves a field
/// unchanged.
pub async fn update(pool: &Pool, patch: &UpdateOrgSettings) -> Result<OrgSettings, DbError> {
    sqlx::query(
        "UPDATE org_settings SET \
            app_name = COALESCE(?, app_name), \
            allow_local_signup = COALESCE(?, allow_local_signup) \
         WHERE id = 1",
    )
    .bind(&patch.app_name)
    .bind(patch.allow_local_signup)
    .execute(pool)
    .await?;

    get(pool).await
}

pub async fn set_logo_url(pool: &Pool, logo_url: Option<&str>) -> Result<OrgSettings, DbError> {
    sqlx::query("UPDATE org_settings SET logo_url = ? WHERE id = 1")
        .bind(logo_url)
        .execute(pool)
        .await?;

    get(pool).await
}
