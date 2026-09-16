#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("invalid stored JSON for {entity} {id}: {source}")]
    CorruptJson {
        entity: &'static str,
        id: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("{entity} {id} not found")]
    NotFound { entity: &'static str, id: String },
}
