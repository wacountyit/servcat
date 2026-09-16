use std::{collections::HashMap, sync::Arc};

use servcat_connectors::{GlpiConnector, JiraConnector, TicketConnector, WebhookConnector};
use servcat_model::TargetSystem;

use crate::config::ConnectorsConfig;

/// Which target systems this deployment can actually dispatch to, built once
/// at startup from whichever connector credentials are present in the
/// environment. A workflow whose `target_system` has no matching entry here
/// fails dispatch with a clear configuration error rather than a panic.
pub struct ConnectorRegistry {
    connectors: HashMap<TargetSystem, Arc<dyn TicketConnector>>,
}

impl ConnectorRegistry {
    pub fn from_config(config: &ConnectorsConfig) -> Self {
        let mut connectors: HashMap<TargetSystem, Arc<dyn TicketConnector>> = HashMap::new();

        if let Some(url) = &config.webhook_url {
            connectors.insert(
                TargetSystem::Webhook,
                Arc::new(WebhookConnector::new(url.clone(), config.webhook_bearer_token.clone())),
            );
        }

        if let (Some(base_url), Some(email), Some(api_token)) =
            (&config.jira_base_url, &config.jira_email, &config.jira_api_token)
        {
            connectors.insert(
                TargetSystem::Jira,
                Arc::new(JiraConnector::new(base_url.clone(), email.clone(), api_token.clone())),
            );
        }

        if let (Some(base_url), Some(app_token), Some(user_token)) =
            (&config.glpi_base_url, &config.glpi_app_token, &config.glpi_user_token)
        {
            connectors.insert(
                TargetSystem::Glpi,
                Arc::new(GlpiConnector::new(base_url.clone(), app_token.clone(), user_token.clone())),
            );
        }

        for target in [TargetSystem::Webhook, TargetSystem::Jira, TargetSystem::Glpi] {
            if !connectors.contains_key(&target) {
                tracing::info!(?target, "no connector configured for target system");
            }
        }

        Self { connectors }
    }

    pub fn get(&self, target: TargetSystem) -> Option<Arc<dyn TicketConnector>> {
        self.connectors.get(&target).cloned()
    }
}
