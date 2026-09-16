use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde::Deserialize;
use servcat_db::repositories::catalog;
use servcat_model::{NewServiceCatalogItem, Role, ServiceCatalogItem, UpdateServiceCatalogItem};
use uuid::Uuid;

use crate::{auth::AuthUser, error::ApiError, state::AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/catalog", get(list).post(create))
        .route("/catalog/{id}", patch(update))
        .route("/catalog/{id}/deactivate", post(deactivate))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    include_inactive: bool,
}

async fn list(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<ServiceCatalogItem>>, ApiError> {
    // Only admins/agents get to see retired/draft items; everyone else only
    // ever sees what's actually available to request.
    let include_inactive =
        query.include_inactive && matches!(auth_user.0.role, Role::Admin | Role::Agent);
    Ok(Json(catalog::list(&state.pool, include_inactive).await?))
}

async fn create(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Json(new_item): Json<NewServiceCatalogItem>,
) -> Result<Json<ServiceCatalogItem>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    Ok(Json(
        catalog::create(&state.pool, &new_item, auth_user.0.id).await?,
    ))
}

async fn update(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(patch): Json<UpdateServiceCatalogItem>,
) -> Result<Json<ServiceCatalogItem>, ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    let updated = catalog::update(&state.pool, id, &patch)
        .await?
        .ok_or_else(|| ApiError::NotFound("catalog item not found".into()))?;
    Ok(Json(updated))
}

async fn deactivate(
    auth_user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<(), ApiError> {
    auth_user.require_role(&[Role::Admin])?;
    catalog::deactivate(&state.pool, id).await?;
    Ok(())
}
