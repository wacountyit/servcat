//! Storage for org/department seal uploads. Files are written under
//! `AppConfig::uploads_dir` (bind-mounted to a Docker volume in
//! `docker-compose.yml` so they survive `docker compose up -d --build`) and
//! served back publicly by the `/uploads/{*path}` route in `routes::mod`.

use std::path::Path;

use axum::body::Bytes;
use uuid::Uuid;

use crate::error::ApiError;

const MAX_LOGO_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_CONTENT_TYPES: &[(&str, &str)] =
    &[("image/png", "png"), ("image/jpeg", "jpg"), ("image/svg+xml", "svg"), ("image/webp", "webp")];

/// Validates and writes an uploaded seal/logo under `uploads_dir/<subdir>/`,
/// removing whatever file previously occupied that slot (if any -- an org
/// or department only ever has one current logo, and old ones shouldn't
/// accumulate on disk across replacements). Returns the public URL path to
/// store on the row.
pub async fn save_logo(
    uploads_dir: &Path,
    subdir: &str,
    slot: &str,
    content_type: &str,
    bytes: Bytes,
    previous_url: Option<&str>,
) -> Result<String, ApiError> {
    if bytes.len() > MAX_LOGO_BYTES {
        return Err(ApiError::BadRequest("logo must be 5MB or smaller".into()));
    }
    let extension = ALLOWED_CONTENT_TYPES
        .iter()
        .find(|(ct, _)| *ct == content_type)
        .map(|(_, ext)| *ext)
        .ok_or_else(|| ApiError::BadRequest("logo must be PNG, JPEG, SVG, or WebP (set Content-Type accordingly)".into()))?;

    let dir = uploads_dir.join(subdir);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        tracing::error!(error = %e, "failed to create uploads directory");
        ApiError::Internal
    })?;

    delete_logo(uploads_dir, previous_url).await;

    // A fresh random filename per upload (rather than overwriting
    // `slot.<ext>` in place) sidesteps a browser/CDN caching a stale image
    // at the same URL right after a replacement.
    let filename = format!("{slot}-{}.{extension}", Uuid::new_v4());
    let path = dir.join(&filename);
    tokio::fs::write(&path, &bytes).await.map_err(|e| {
        tracing::error!(error = %e, "failed to write uploaded logo");
        ApiError::Internal
    })?;

    Ok(format!("/uploads/{subdir}/{filename}"))
}

/// Best-effort delete: a missing file (already removed, or a URL that
/// predates this layout) shouldn't fail whatever request is replacing or
/// clearing it.
pub async fn delete_logo(uploads_dir: &Path, url: Option<&str>) {
    let Some(url) = url else { return };
    let Some(relative) = url.strip_prefix("/uploads/") else { return };
    let _ = tokio::fs::remove_file(uploads_dir.join(relative)).await;
}
