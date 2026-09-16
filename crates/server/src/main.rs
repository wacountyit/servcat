mod auth;
mod config;
mod connector_registry;
mod error;
mod expiry_handler;
mod orchestrator;
mod routes;
mod sso;
mod state;
mod uploads;

use std::{sync::Arc, time::Duration};

use servcat_db::repositories::{org_settings, users};
use servcat_model::{NewUser, Role};
use tokio::signal;
use tracing_subscriber::EnvFilter;

use crate::{
    config::AppConfig, connector_registry::ConnectorRegistry, expiry_handler::ServerExpiryHandler, sso::SsoService,
    state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Ignore a missing .env file -- real deployments set these as actual
    // process environment variables instead.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Arc::new(AppConfig::from_env()?);
    let pool = servcat_db::connect_and_migrate(&config.database_url).await?;

    bootstrap_admin(&pool, &config).await?;
    org_settings::seed_default(&pool, config.bootstrap_app_name.as_deref(), config.bootstrap_allow_local_signup).await?;

    let state = AppState {
        pool,
        connectors: Arc::new(ConnectorRegistry::from_config(&config.connectors)),
        notifier: servcat_approvals::default_notifier(),
        uploads_dir: config.uploads_dir.clone(),
        sso: config.sso.clone().map(|sso_config| Arc::new(SsoService::new(sso_config))),
        config,
    };

    servcat_approvals::spawn_expiry_poller(
        state.pool.clone(),
        Arc::new(ServerExpiryHandler(state.clone())),
        Duration::from_secs(60),
    );

    let app = routes::build_router(state.clone());
    let listener = tokio::net::TcpListener::bind(state.config.bind_addr).await?;
    tracing::info!(addr = %state.config.bind_addr, "servcat-server listening");

    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;

    Ok(())
}

/// Creates the configured bootstrap admin the first time the server starts
/// against a fresh database (no-op once any admin exists). Meant to be
/// rotated/disabled once real accounts (local or SSO) take over -- see
/// `.env.example`.
async fn bootstrap_admin(pool: &servcat_db::Pool, config: &AppConfig) -> anyhow::Result<()> {
    let (Some(email), Some(password)) = (&config.bootstrap_admin_email, &config.bootstrap_admin_password) else {
        return Ok(());
    };

    if users::get_by_email(pool, email).await?.is_some() {
        return Ok(());
    }
    if !users::list(pool, true).await?.iter().all(|u| u.role != Role::Admin) {
        return Ok(());
    }

    let password_hash = auth::hash_password(password)?;
    users::create(
        pool,
        &NewUser {
            email: email.clone(),
            display_name: "Bootstrap Administrator".to_string(),
            password: None,
            external_idp_subject: None,
            department_id: None,
            manager_user_id: None,
            role: Role::Admin,
        },
        Some(&password_hash),
    )
    .await?;

    tracing::warn!(%email, "created bootstrap admin account -- rotate its password and/or disable it once real admins exist");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async { signal::ctrl_c().await.expect("failed to install Ctrl+C handler") };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
