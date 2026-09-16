use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
