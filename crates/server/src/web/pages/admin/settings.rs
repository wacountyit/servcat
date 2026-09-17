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
    timezones: Vec<TimezoneOption>,
    error: Option<String>,
}

/// `selected` is precomputed here rather than compared in the template --
/// Askama passes call/comparison operands by reference regardless of the
/// field's actual type, which makes a plain `tz == timezone` between a
/// loop variable and a `String` field brittle (see `is_selected_parent` in
/// `admin/departments.rs` for the same gotcha). A plain struct field read
/// via `tz.selected` sidesteps it the same way `def.is_published` already
/// does elsewhere (`admin/workflows.html`).
struct TimezoneOption {
    name: &'static str,
    selected: bool,
}

async fn show(State(state): State<AppState>, WebUser(user): WebUser) -> Result<Response, WebError> {
    require_admin(&user)?;
    let layout = Layout::load(&state, &user, "admin").await?;
    let settings = org_settings::get(&state.pool).await?;
    let timezones = timezone_options(&settings.timezone);
    Ok(html(SettingsTemplate {
        layout,
        admin_section: "settings",
        app_name: settings.app_name,
        allow_local_signup: settings.allow_local_signup,
        sso_enabled: state.sso.is_some(),
        timezones,
        error: None,
    }))
}

fn timezone_options(current: &str) -> Vec<TimezoneOption> {
    let mut names: Vec<&'static str> = crate::timezone::all_names().collect();
    names.sort_unstable();
    names
        .into_iter()
        .map(|name| TimezoneOption {
            name,
            selected: name == current,
        })
        .collect()
}

#[derive(Deserialize)]
struct UpdateSettingsForm {
    app_name: String,
    #[serde(default)]
    allow_local_signup: bool,
    timezone: String,
}

async fn update(
    State(state): State<AppState>,
    WebUser(user): WebUser,
    Form(form): Form<UpdateSettingsForm>,
) -> Result<Response, WebError> {
    require_admin(&user)?;

    if !crate::timezone::is_valid(&form.timezone) {
        let layout = Layout::load(&state, &user, "admin").await?;
        let timezones = timezone_options(&form.timezone);
        return Ok(html(SettingsTemplate {
            layout,
            admin_section: "settings",
            app_name: form.app_name,
            allow_local_signup: form.allow_local_signup,
            sso_enabled: state.sso.is_some(),
            timezones,
            error: Some("Please choose a valid timezone from the list.".to_string()),
        }));
    }

    org_settings::update(
        &state.pool,
        &UpdateOrgSettings {
            app_name: Some(form.app_name),
            allow_local_signup: Some(form.allow_local_signup),
            timezone: Some(form.timezone),
        },
    )
    .await?;
    Ok(Redirect::to("/admin/settings").into_response())
}
