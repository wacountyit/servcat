//! Public static file serving for uploaded org/department seals
//! (`crate::uploads` writes them, this serves them back). Hand-rolled
//! rather than `tower_http::services::ServeDir` -- deliberately minimal
//! since this only ever serves the small, fixed set of image files this
//! server itself wrote under `uploads_dir`.

use axum::{
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::state::AppState;

pub async fn serve(Path(path): Path<String>, State(state): State<AppState>) -> Response {
    if path
        .split('/')
        .any(|segment| segment == ".." || segment.is_empty())
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    let full_path = state.uploads_dir.join(&path);
    match tokio::fs::read(&full_path).await {
        Ok(bytes) => ([(header::CONTENT_TYPE, content_type_for(&path))], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn content_type_for(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}
