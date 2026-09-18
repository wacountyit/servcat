//! Server-rendered web UI (Askama templates), mounted alongside the JSON API
//! (nested under `/api`, see `crate::routes::build_router`) at the site
//! root. Authenticates browsers via httponly session cookies bridged to the
//! same JWT/refresh-token machinery the JSON API uses -- see `session`.

mod layout;
mod pages;
mod render;
mod session;

use axum::{
    Router,
    http::header,
    middleware,
    response::{IntoResponse, Response},
    routing::get,
};

use crate::state::AppState;

const STYLE_CSS: &str = include_str!("../../static/style.css");
const WORKFLOW_BUILDER_JS: &str = include_str!("../../static/workflow_builder.js");

pub fn routes(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/", get(pages::dashboard::show))
        .merge(pages::catalog::routes())
        .merge(pages::requests::routes())
        .merge(pages::approvals::routes())
        .merge(pages::admin::routes())
        .layer(middleware::from_fn_with_state(
            state,
            session::require_session,
        ));

    Router::new()
        .route("/static/style.css", get(style_css))
        .route("/static/workflow_builder.js", get(workflow_builder_js))
        .merge(pages::auth::routes())
        .merge(protected)
}

async fn style_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        STYLE_CSS,
    )
        .into_response()
}

async fn workflow_builder_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        WORKFLOW_BUILDER_JS,
    )
        .into_response()
}
