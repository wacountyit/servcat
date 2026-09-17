use std::sync::Arc;

use async_trait::async_trait;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, header::ContentType},
    transport::smtp::authentication::Credentials,
};
use servcat_model::{PendingApproval, User};

#[derive(Debug, Clone, Copy)]
pub enum SmtpSecurity {
    /// Plaintext connection upgraded via `STARTTLS` (the common case for
    /// port 587, e.g. Microsoft 365's `smtp.office365.com`).
    StartTls,
    /// TLS from the first byte (the common case for port 465).
    ImplicitTls,
    /// No encryption at all. Only appropriate for an internal relay that's
    /// not reachable off-host/off-network (e.g. a local Exchange connector
    /// that allow-lists this server's IP instead of requiring auth).
    None,
}

#[derive(Debug, Clone)]
pub struct MailerConfig {
    pub host: String,
    pub port: Option<u16>,
    pub security: SmtpSecurity,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    /// Base URL of this deployment (e.g. `https://servcat.example.com`, no
    /// trailing slash), used by callers that link back to a page here (an
    /// approval notification's `/approvals` link, a password-reset email's
    /// `/reset-password?token=...` link). Callers must handle it being
    /// unset themselves; `Mailer` doesn't require it.
    pub app_base_url: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MailerBuildError {
    #[error("invalid SMTP relay configuration: {0}")]
    Smtp(#[from] lettre::transport::smtp::Error),
    #[error("invalid SMTP from-address: {0}")]
    FromAddress(#[from] lettre::address::AddressError),
}

#[derive(Debug, thiserror::Error)]
pub enum MailerSendError {
    #[error("invalid recipient address: {0}")]
    InvalidAddress(#[from] lettre::address::AddressError),
    #[error("failed to build email: {0}")]
    Build(#[from] lettre::error::Error),
    #[error("failed to send email: {0}")]
    Send(#[from] lettre::transport::smtp::Error),
}

/// A configured SMTP relay this deployment can send plain-text email
/// through. General-purpose -- used both by `SmtpNotifier` (approval pings)
/// and directly by the server's password-reset flow, so the relay/transport
/// setup only lives in one place.
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    app_base_url: Option<String>,
}

impl Mailer {
    pub fn new(cfg: MailerConfig) -> Result<Self, MailerBuildError> {
        let mut builder = match cfg.security {
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host)?
            }
            SmtpSecurity::ImplicitTls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host)?,
            SmtpSecurity::None => {
                AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host)
            }
        };
        if let Some(port) = cfg.port {
            builder = builder.port(port);
        }
        if let (Some(username), Some(password)) = (cfg.username, cfg.password) {
            builder = builder.credentials(Credentials::new(username, password));
        }

        let from: Mailbox = match cfg.from_name {
            Some(name) => format!("{name} <{}>", cfg.from_address).parse()?,
            None => cfg.from_address.parse()?,
        };

        Ok(Self {
            transport: builder.build(),
            from,
            app_base_url: cfg.app_base_url,
        })
    }

    pub fn app_base_url(&self) -> Option<&str> {
        self.app_base_url.as_deref()
    }

    pub async fn send_plain_text(
        &self,
        to: &str,
        subject: &str,
        body: String,
    ) -> Result<(), MailerSendError> {
        let to: Mailbox = to.parse()?;
        let message = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body)?;
        self.transport.send(message).await?;
        Ok(())
    }
}

/// Everything a notifier needs to render a useful "you have an approval
/// waiting" message. Bundled into one struct (rather than more `&self`
/// parameters) since which fields a given implementation actually uses
/// varies -- `LoggingNotifier` logs all of it, an SMS-length notifier might
/// only use `catalog_item_name`.
pub struct NewApprovalNotification<'a> {
    pub approval: &'a PendingApproval,
    pub approver: &'a User,
    pub requester: &'a User,
    pub catalog_item_name: &'a str,
    /// The org's admin-configured display timezone (IANA name, e.g.
    /// `"America/Chicago"`, from `org_settings.timezone`), used to render
    /// `approval.expires_at` -- every stored timestamp is UTC regardless.
    pub org_timezone: &'a str,
}

/// Renders `dt` in `tz_name`, falling back to UTC if `tz_name` isn't a
/// recognized IANA name (shouldn't happen -- `org_settings.timezone` is
/// validated on write -- but a notification must never panic over it).
fn format_local(dt: chrono::DateTime<chrono::Utc>, tz_name: &str) -> String {
    match tz_name.parse::<chrono_tz::Tz>() {
        Ok(tz) => dt
            .with_timezone(&tz)
            .format("%Y-%m-%d %H:%M %Z")
            .to_string(),
        Err(_) => dt.format("%Y-%m-%d %H:%M UTC").to_string(),
    }
}

/// Fired once when a `PendingApproval` is created. Nothing in the approval
/// flow depends on notification actually succeeding (a missed notification
/// does not block an approver from acting, since they can still see it in
/// their in-app approvals inbox) -- implementations should log and swallow
/// their own failures rather than propagate them.
#[async_trait]
pub trait ApprovalNotifier: Send + Sync {
    async fn notify_new_approval(&self, ctx: &NewApprovalNotification<'_>);
}

/// Fallback notifier used when no `Mailer` is configured. An approver's only
/// way to find out about a pending approval is then checking the in-app
/// `/approvals` inbox themselves.
pub struct LoggingNotifier;

#[async_trait]
impl ApprovalNotifier for LoggingNotifier {
    async fn notify_new_approval(&self, ctx: &NewApprovalNotification<'_>) {
        tracing::info!(
            approval_id = %ctx.approval.id,
            approver_email = %ctx.approver.email,
            requester_email = %ctx.requester.email,
            catalog_item_name = %ctx.catalog_item_name,
            step_id = %ctx.approval.step_id,
            "approval is pending, but no SMTP mailer is configured -- not emailed anywhere"
        );
    }
}

/// Emails the approver via a shared `Mailer` when a `PendingApproval` is
/// created for them.
pub struct SmtpNotifier {
    mailer: Arc<Mailer>,
}

impl SmtpNotifier {
    pub fn new(mailer: Arc<Mailer>) -> Self {
        Self { mailer }
    }
}

#[async_trait]
impl ApprovalNotifier for SmtpNotifier {
    async fn notify_new_approval(&self, ctx: &NewApprovalNotification<'_>) {
        let mut body = format!(
            "{} requested \"{}\" and it is now waiting on your approval.\n",
            ctx.requester.display_name, ctx.catalog_item_name
        );
        if let Some(expires_at) = ctx.approval.expires_at {
            body.push_str(&format!(
                "This request will automatically be rejected on {} if it isn't decided before then.\n",
                format_local(expires_at, ctx.org_timezone)
            ));
        }
        if let Some(base_url) = self.mailer.app_base_url() {
            body.push_str(&format!(
                "\nDecide it here: {}/approvals\n",
                base_url.trim_end_matches('/')
            ));
        }

        if let Err(err) = self
            .mailer
            .send_plain_text(
                &ctx.approver.email,
                &format!("Approval needed: {}", ctx.catalog_item_name),
                body,
            )
            .await
        {
            tracing::error!(
                error = %err,
                approval_id = %ctx.approval.id,
                approver_email = %ctx.approver.email,
                "failed to send approval notification email"
            );
        }
    }
}
