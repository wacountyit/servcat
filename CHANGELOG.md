# Changelog

All notable changes to this project are documented here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); this project has
not cut a `0.1.0` release yet, so everything so far is under "Unreleased."

## [Unreleased]

### Added

- Workflow engine (`workflow-engine`): a pure graph interpreter over
  admin-authored `WorkflowDefinition`s (`Question`, `Branch`,
  `WaitForApproval`, `SubmitTicket`, `End` steps).
- Ticket connectors (`connectors`): `TicketConnector` trait with Jira,
  GLPI, and generic webhook implementations, plus a Tera-template-based
  field-mapping renderer for translating collected answers into each
  target system's required shape.
- Approvals (`approvals`): approver resolution (`Static`,
  `ManagerOfRequester`, `RoleInDepartment`), a (log-only) notifier, and a
  background poller for expired approvals.
- Core domain and schema: departments (with a parent/child hierarchy),
  users, workflow definitions and catalog items, workflow instances,
  pending approvals, tickets, and an append-only audit log
  (`migrations/0001` through `0005`).
- Axum API (`server`) exposing all of the above, with local email/password
  authentication (Argon2id, short-lived JWT access tokens, single-use
  hashed refresh tokens).
- Microsoft Entra ID (Azure AD) single sign-on: authorization-code + PKCE
  flow, ID token validation against Entra ID's JWKS (signature, issuer,
  audience, nonce), and a one-time handoff-code exchange so Entra ID's
  tokens never reach the browser directly (`crates/server/src/sso.rs`,
  `routes/sso.rs`).
- Local self-registration (`POST /auth/register`), off by default and
  gated by a runtime-editable `org_settings.allow_local_signup` flag
  rather than an env var requiring a redeploy to change
  (`migrations/0006_org_settings.sql`).
- Organization branding: an editable org display name and org-wide seal,
  plus an optional per-department seal override
  (`migrations/0007_department_logo.sql`), served through a public
  `GET /config` endpoint for a not-yet-authenticated frontend to read.
- Docker deployment: a multi-stage `Dockerfile` for this multi-crate
  workspace, a `docker-compose.yml` (MariaDB, the app, and an optional
  Caddy reverse proxy behind a Compose profile), and `install.sh`, an
  interactive installer that generates secrets, prompts for the host
  port/reverse-proxy/CORS/bootstrap-admin/SSO settings, and brings the
  stack up.
- Operational scripts: `scripts/migrate.sh` (sqlx-cli wrapper),
  `scripts/backup-db.sh`, and `scripts/restore-db.sh`.
- Project documentation: this file, plus `ARCHITECTURE.md`, `SECURITY.md`,
  `TESTING.md`, and `CONTRIBUTING.md`.
- CI/CD (`.github/workflows/`): `ci.yml` (`cargo fmt`/`check`/`clippy -D
  warnings`/`test`, plus a Docker build smoke test, on every PR and push to
  `main`), `docker-publish.yml` (builds and pushes to GHCR on `main` and
  version tags, no secrets to configure), and `release.yml` (a version tag
  builds a standalone release binary and cuts a GitHub Release).
- `Mailer` (`crates/approvals`), a shared SMTP relay client
  (`SMTP_HOST`/`SMTP_PORT`/`SMTP_SECURITY`/`SMTP_USERNAME`/`SMTP_PASSWORD`/
  `SMTP_FROM_ADDRESS`/`SMTP_FROM_NAME`, plus `APP_BASE_URL` for links back
  to this deployment), backing two features:
  - `SmtpNotifier`: emails an approver when a `PendingApproval` is created
    for them, with a link straight to `/approvals`. Falls back to the
    existing log-only `LoggingNotifier` when SMTP isn't configured, same
    as before.
  - Self-service password reset: `POST /auth/password-reset/request`/
    `.../confirm` (and the web UI's `/forgot-password` ->
    `/reset-password` pages), backed by single-use, 30-minute tokens
    (`password_resets`, migration `0008`) hashed the same way as
    `sessions.refresh_token_hash`. Unavailable entirely (404 from the
    API, hidden/redirected in the web UI) unless SMTP is configured.
- `org_settings.timezone` (migration `0009`), an admin-only setting
  (`/admin/settings`, `PATCH /admin/settings`) controlling how timestamps
  are *displayed* in the web UI and approval notification emails --
  stored/transmitted timestamps remain UTC everywhere regardless.
  Defaults to `UTC`; rejects anything that isn't a recognized IANA
  timezone name.
- `install.sh` now interactively prompts for SMTP (host, port, security
  mode, optional credentials, from-address/name), mirroring the existing
  Entra ID SSO prompt -- optional, blank host skips it (writing blank
  `SMTP_*` vars so a re-run doesn't re-prompt), same as before this was
  only configurable by hand-editing `.env` after install.
- Admin-only audit log viewer: `GET /api/audit-log` (paginated,
  `?page=`/`?page_size=`) and `/admin/audit-log` in the web UI, resolving
  `actor_user_id` to a display name and rendering timestamps through the
  new `org_settings.timezone` setting. `audit_log` itself was already
  populated (user create/update/deactivate) but had no way to read it
  back short of querying the database directly.

### Fixed

- The requester-facing `/requests/{id}` page never surfaced the resulting
  ticket once a `SubmitTicket` step dispatched successfully (or failed),
  even though `tickets.external_ticket_id`/`external_ticket_url` were
  already captured in the database. It now shows a link to the external
  ticket when the connector returns a URL, the bare reference otherwise,
  and a plain "contact IT support" message (with the raw connector error
  visible to admins/agents only) on dispatch failure.
- `install.sh` computed `public_base_url` for the reverse-proxy/HTTPS setup
  step but never wrote it to `.env` as `APP_BASE_URL`, so a fresh install
  would silently ship with broken/relative links in approval-notification
  and password-reset emails until an admin noticed and added it by hand.
  Now written automatically whenever the installer resolved a real
  address (skipped, with an explanation, for the bare-IP Caddy branch,
  which only has a `<this-host-ip>` placeholder at that point).
- README.md/SECURITY.md described `audit_log` as covering "catalog/
  workflow changes ... approval decisions" alongside user/role changes;
  only the latter was ever actually recorded. Corrected to describe what's
  actually wired up today, and noted in "Known gaps."
- MariaDB's host-side port was hardcoded to `3306` in `docker-compose.yml`
  with no way to change it, so `docker compose up` would fail outright
  (`port is already allocated`) on a host already running some other
  MySQL/MariaDB instance -- the same class of problem `APP_PORT` already
  solved for the app's own port. `install.sh` now checks 3306 the same
  way it already checks 8080 and prompts for an alternate `DB_PORT` if
  it's taken; `docker-compose.yml`'s port mapping reads `DB_PORT` (default
  3306). Since `DATABASE_URL` (used by `scripts/migrate.sh` on the host)
  embeds this port directly, port selection now happens *before* `.env`
  is generated instead of after, and re-running `install.sh` against an
  older `.env` that predates `DB_PORT` backfills it and fixes up
  `DATABASE_URL`'s port to match.
- `docker-compose.yml`'s `app` service never actually passed
  `SMTP_*`/`APP_BASE_URL` through to the container's environment, even
  though `.env` (and now `install.sh`'s SMTP prompt) had them -- approval
  notifications and password reset would silently stay disabled under
  `docker compose up` regardless of `.env` being configured correctly.
- Documented concrete rate-limiting options (nginx `limit_req` snippet,
  the bundled Caddy's lack of built-in rate limiting, and the option of
  just not exposing this publicly) in SECURITY.md's production hardening
  checklist, in place of a bare "put a rate limit in front of it" note.
- `docker-compose.yml`'s MariaDB healthcheck lacked a startup grace
  period, so a brand-new data volume's first-time initialization (which
  can take well over a minute) could get marked unhealthy before it ever
  finished, blocking the `app` container from starting. Added a
  120-second `start_period`.
- `.gitignore`'s original backup-artifact pattern (`*.sql`) would have
  also ignored `migrations/*.sql`, which is tracked source, not backup
  output. Scoped to the actual compressed formats `scripts/backup-db.sh`
  produces (`*.sql.gz`, `*.sql.xz`, `*.dump`) with an explicit
  `!/migrations/*.sql` safety net.
- `Cargo.toml` declared `license = "MIT"` while the repository's actual
  `LICENSE` file was AGPL-3.0. Corrected `Cargo.toml` to match the license
  that's actually in effect (`AGPL-3.0-or-later`).

### Changed

- The host-side Docker port is now configurable (`APP_PORT` in `.env`,
  prompted by `install.sh` with automatic detection of whether 8080 is
  already taken) instead of hardcoded, since the app container always
  listens on 8080 internally regardless of the host mapping.
- Ran `cargo fmt --all` across the whole workspace (44 files, whitespace
  and import-ordering only, no behavior change; verified by an identical
  `cargo check`/`test`/`clippy` result before and after) so `ci.yml`'s
  `cargo fmt --all -- --check` job could become a required gate instead of
  an advisory one.
