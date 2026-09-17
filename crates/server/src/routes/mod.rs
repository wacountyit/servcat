mod approvals;
mod auth;
mod catalog;
mod departments;
mod health;
mod instances;
mod settings;
mod sso;
mod uploads;
mod users;
mod workflow_definitions;

use std::time::Duration;

use axum::{Router, http::HeaderValue, http::StatusCode, routing::get};
use tower_http::{cors::CorsLayer, timeout::TimeoutLayer, trace::TraceLayer};

use crate::state::AppState;

/// The JSON API. Nested under `/api` in `build_router` so it can share the
/// same origin (and therefore cookies) with the server-rendered web UI in
/// `crate::web` without any path collisions between the two -- e.g. the API's
/// `/catalog` and the web UI's page at `/catalog` used to be the same path.
fn api_router() -> Router<AppState> {
    Router::new()
        .merge(auth::routes())
        .merge(sso::routes())
        .merge(settings::routes())
        .merge(users::routes())
        .merge(departments::routes())
        .merge(catalog::routes())
        .merge(workflow_definitions::routes())
        .merge(instances::routes())
        .merge(approvals::routes())
}

pub fn build_router(state: AppState) -> Router {
    let cors = build_cors_layer(&state.config.cors_allowed_origins);

    Router::new()
        .merge(health::routes())
        .route("/uploads/{*path}", get(uploads::serve))
        .nest("/api", api_router())
        .merge(crate::web::routes(state.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(cors)
        .with_state(state)
}

/// No allowed origins configured means no cross-origin browser access at
/// all (same-origin and non-browser clients are unaffected) -- safer default
/// than an accidental wildcard.
fn build_cors_layer(allowed_origins: &[String]) -> CorsLayer {
    let origins: Vec<HeaderValue> = allowed_origins
        .iter()
        .filter_map(|o| o.parse().ok())
        .collect();
    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
}
