use std::{path::PathBuf, sync::Arc};

use servcat_approvals::ApprovalNotifier;
use servcat_db::Pool;

use crate::{config::AppConfig, connector_registry::ConnectorRegistry, sso::SsoService};

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub config: Arc<AppConfig>,
    pub connectors: Arc<ConnectorRegistry>,
    pub notifier: Arc<dyn ApprovalNotifier>,
    pub uploads_dir: PathBuf,
    /// `None` when Entra ID SSO env vars aren't configured -- `/auth/sso/*`
    /// routes then respond 404 instead of panicking.
    pub sso: Option<Arc<SsoService>>,
}
