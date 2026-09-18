mod auth;
mod catalog_seed;
mod config;
mod connector_registry;
mod error;
mod expiry_handler;
mod orchestrator;
mod routes;
mod sso;
mod state;
mod timezone;
mod uploads;
mod web;

use std::{sync::Arc, time::Duration};

use servcat_db::repositories::{catalog, org_settings, users};
use servcat_model::{NewUser, Role};
use tokio::signal;
use tracing_subscriber::EnvFilter;

use crate::{
    config::AppConfig, connector_registry::ConnectorRegistry, expiry_handler::ServerExpiryHandler,
    sso::SsoService, state::AppState,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Ignore a missing .env file -- real deployments set these as actual
    // process environment variables instead.
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Arc::new(AppConfig::from_env()?);
    let pool = servcat_db::connect_and_migrate(&config.database_url).await?;

    bootstrap_admin(&pool, &config).await?;
    auto_seed_catalog(&pool).await?;
    org_settings::seed_default(
        &pool,
        config.bootstrap_app_name.as_deref(),
        config.bootstrap_allow_local_signup,
    )
    .await?;

    let mailer: Option<Arc<servcat_approvals::Mailer>> = match &config.smtp {
        Some(smtp) => Some(Arc::new(servcat_approvals::Mailer::new(
            servcat_approvals::MailerConfig {
                host: smtp.host.clone(),
                port: smtp.port,
                security: smtp.security,
                username: smtp.username.clone(),
                password: smtp.password.clone(),
                from_address: smtp.from_address.clone(),
                from_name: smtp.from_name.clone(),
                app_base_url: config.app_base_url.clone(),
            },
        )?)),
        None => {
            tracing::warn!(
                "SMTP is not configured (set SMTP_HOST and SMTP_FROM_ADDRESS) -- approval \
                 notifications will only be written to the server log, and self-service \
                 password reset is disabled"
            );
            None
        }
    };

    let notifier: Arc<dyn servcat_approvals::ApprovalNotifier> = match &mailer {
        Some(mailer) => Arc::new(servcat_approvals::SmtpNotifier::new(mailer.clone())),
        None => servcat_approvals::default_notifier(),
    };

    let state = AppState {
        pool,
        connectors: Arc::new(ConnectorRegistry::from_config(&config.connectors)),
        notifier,
        mailer,
        uploads_dir: config.uploads_dir.clone(),
        sso: config
            .sso
            .clone()
            .map(|sso_config| Arc::new(SsoService::new(sso_config))),
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

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Creates the configured bootstrap admin the first time the server starts
/// against a fresh database (no-op once any admin exists). Meant to be
/// rotated/disabled once real accounts (local or SSO) take over -- see
/// `.env.example`.
async fn bootstrap_admin(pool: &servcat_db::Pool, config: &AppConfig) -> anyhow::Result<()> {
    let (Some(email), Some(password)) = (
        &config.bootstrap_admin_email,
        &config.bootstrap_admin_password,
    ) else {
        return Ok(());
    };

    if users::get_by_email(pool, email).await?.is_some() {
        return Ok(());
    }
    if !users::list(pool, true)
        .await?
        .iter()
        .all(|u| u.role != Role::Admin)
    {
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

/// Loads the starter IT service catalog the first time the server starts
/// against a database with no catalog items at all (no-op afterward, even
/// if every seeded item is later edited or deactivated -- see
/// `catalog_seed` for the by-slug idempotency this relies on). Attributed
/// to whichever admin account happens to exist yet; skipped entirely if
/// none does yet, same as `bootstrap_admin` requires configured
/// credentials to create one.
async fn auto_seed_catalog(pool: &servcat_db::Pool) -> anyhow::Result<()> {
    if !catalog::list(pool, true).await?.is_empty() {
        return Ok(());
    }
    let Some(admin) = users::list(pool, true)
        .await?
        .into_iter()
        .find(|u| u.role == Role::Admin)
    else {
        return Ok(());
    };

    let summary = catalog_seed::run(pool, admin.id).await?;
    tracing::info!(
        added = summary.added,
        skipped = summary.skipped,
        "seeded starter IT service catalog"
    );
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler")
    };

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
