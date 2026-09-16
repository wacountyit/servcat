mod error;
pub mod repositories;

pub use error::DbError;
pub use sqlx::MySqlPool as Pool;

/// Opens the connection pool and runs any pending migrations embedded at
/// build time from `../../migrations` (relative to this crate). Call once at
/// server startup.
pub async fn connect_and_migrate(database_url: &str) -> Result<Pool, DbError> {
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    Ok(pool)
}
