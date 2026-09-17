use askama::Template;
use axum::{
    Form, Router,
    extract::State,
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use serde::Deserialize;
use servcat_db::repositories::org_settings;
use servcat_model::UpdateOrgSettings;

use crate::{
    state::AppState,
    web::{
        layout::Layout,
        render::{WebError, html},
        session::WebUser,
    },
};

use super::require_admin;

pub fn routes() -> Router<AppState> {
    Router::new().route("/admin/settings", get(show).post(update))
}

#[derive(Template)]
#[template(path = "admin/settings.html")]
struct SettingsTemplate {
    layout: Layout,
    admin_section: &'static str,
    app_name: String,
    allow_local_signup: bool,
    sso_enabled: bool,
}

async fn show(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let settings = org_settings::get(&state.pool).await?;
    Ok(html(SettingsTemplate {
        layout,
        admin_section: "settings",
        app_name: settings.app_name,
        allow_local_signup: settings.allow_local_signup,
        sso_enabled: state.sso.is_some(),
    }))
}

#[derive(Deserialize)]
struct UpdateSettingsForm {
    app_name: String,
    #[serde(default)]
    allow_local_signup: bool,
}

async fn update(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<UpdateSettingsForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;
    org_settings::update(
        &state.pool,
        &UpdateOrgSettings {
            app_name: Some(form.app_name),
            allow_local_signup: Some(form.allow_local_signup),
        },
    )
    .await?;
    Ok(Redirect::to("/admin/settings").into_response())
}
