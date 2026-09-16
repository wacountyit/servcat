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

### Fixed

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
