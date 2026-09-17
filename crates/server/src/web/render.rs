//! Shared response helpers for the server-rendered web UI: turning an Askama
//! template into an HTML `Response`, and a page-oriented error type that
//! renders a human-readable error page instead of the JSON API's `{"error":
//! ...}` body.

use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};

use crate::error::ApiError;

/// Renders `template`, or a plain-text 500 (logged) if rendering itself
/// fails -- which only happens for a bug in the template, never user input.
pub fn html<T: Template>(template: T) -> Response {
    match template.render() {
        Ok(body) => Html(body).into_response(),
        Err(err) => {
            tracing::error!(error = %err, "template render failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "template render error").into_response()
        }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    status: u16,
    message: &'a str,
}

/// Page-handler error type. Wraps the same `ApiError` the JSON API already
/// produces (orchestrator/repository calls return it directly) but renders
/// an HTML error page, and turns an expired/missing session into a redirect
/// to the login page rather than a 401 body.
pub struct WebError(pub ApiError);

impl From<ApiError> for WebError {
    fn from(err: ApiError) -> Self {
        WebError(err)
    }
}

impl From<servcat_db::DbError> for WebError {
    fn from(err: servcat_db::DbError) -> Self {
        WebError(ApiError::from(err))
    }
}

impl From<servcat_approvals::ApprovalsError> for WebError {
    fn from(err: servcat_approvals::ApprovalsError) -> Self {
        WebError(ApiError::from(err))
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        if matches!(self.0, ApiError::Unauthorized) {
            return Redirect::to("/login").into_response();
        }

        let status = match &self.0 {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let message = self.0.to_string();
        (
            status,
            html(ErrorTemplate {
                status: status.as_u16(),
                message: &message,
            }),
        )
            .into_response()
    }
}
