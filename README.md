# ServCat

A self-hosted IT service catalog / self-service portal, in the spirit of
ServiceNow's Employee Self-Service Catalog or GLPI's self-service module -
lightweight, org-customizable, and designed to front an existing ticketing
system (Jira, GLPI, Freshservice, or a generic webhook) rather than reinvent
one.

This repo is the Rust backend and its web UI: a workflow interpreter, a
MariaDB-backed domain model, an Axum JSON API, and a server-rendered (Askama)
web frontend for requesters, approvers, and admins, all in one binary. See
"Frontend" below for how the web UI is built and how a Tauri desktop shell
fits in.

More docs:

- [ARCHITECTURE.md](ARCHITECTURE.md) - crate layout, request lifecycle, data model
- [SECURITY.md](SECURITY.md) - reporting a vulnerability, security posture, production hardening checklist
- [TESTING.md](TESTING.md) - automated tests and the manual QA checklist
- [CONTRIBUTING.md](CONTRIBUTING.md) - dev setup, conventions, PR expectations
- [CHANGELOG.md](CHANGELOG.md) - notable changes by version

## Architecture

```
crates/
|-- model/             Shared domain types (User, WorkflowDefinition, ...)
|-- db/                sqlx/MariaDB pool + migrations + repositories
|-- workflow-engine/   Pure, synchronous graph interpreter (no I/O)
|-- connectors/        TicketConnector trait + Jira/GLPI/Webhook + field-mapping templates
|-- approvals/         Approver resolution, notification, expiry polling
`-- server/            Axum API: auth, routes, and the orchestrator that
                        wires the engine to connectors/approvals/db
migrations/            Versioned MariaDB schema (sqlx migrate)
```

A `WorkflowDefinition` is an admin-authored graph of `Step`s (`Question`,
`Branch`, `WaitForApproval`, `SubmitTicket`, `End`). `workflow-engine`
interprets that graph but performs no I/O itself: it returns an `Outcome`
(`AwaitingAnswer`, `AwaitingApproval`, `ReadyToSubmitTicket`, `Finished`) and
lets `server::orchestrator` do the actual database writes, approver
resolution/notification, and connector dispatch. This keeps the interpreter
trivially unit-testable (see `crates/workflow-engine/src/lib.rs` tests) and
keeps side effects in one auditable place.

Each `WorkflowDefinition` carries an optional `field_mapping`: a map from a
target system's field name (dot-paths like `"project.key"` nest into JSON,
e.g. for Jira) to a Tera template rendered against the collected answers,
the requester's profile, and instance metadata. That's what makes adding a
new catalog item against a differently-configured Jira project or GLPI
entity type a configuration change, not a code change.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full request lifecycle, data
model, and deployment topology.

## Getting started

### Docker (recommended)

`./install.sh` generates a `.env` with strong random secrets, walks through
reverse-proxy/HTTPS, the host port, frontend CORS origin, a bootstrap admin
account, the local sign-up toggle, and (optionally) Microsoft Entra ID SSO
and SMTP (for approval-notification emails and self-service password
reset), then runs `docker compose up -d --build`. Re-running
`docker compose up -d --build` after a code change only rebuilds/recreates
the `app` container; MariaDB keeps running, so there's no database downtime
on a redeploy.

See `scripts/migrate.sh` (sqlx-cli, run from the host against the
Docker-exposed `127.0.0.1:<DB_PORT>`, `3306` unless `install.sh` picked a
different port because something else already had 3306),
`scripts/backup-db.sh`, and `scripts/restore-db.sh` for day-to-day
operations.

### Running directly

1. Copy `.env.example` to `.env` and fill in real values: at minimum
   `DATABASE_URL` and a generated `JWT_SIGNING_SECRET`
   (`openssl rand -base64 48`).
2. Start MariaDB (10.6+ recommended) and create the database/user referenced
   by `DATABASE_URL`.
3. `cargo run -p servcat-server`. This applies all pending migrations on
   startup, seeds the `org_settings` row (`APP_NAME`/`ALLOW_LOCAL_SIGNUP`,
   only on the very first run), and, if `BOOTSTRAP_ADMIN_EMAIL`/
   `BOOTSTRAP_ADMIN_PASSWORD` are set and no admin exists yet, creates that
   admin account. Rotate or disable it once real accounts exist.
4. `cargo test --workspace` runs the engine's graph tests and the
   field-mapping renderer's tests (no database required).

Connectors only register if their env vars are fully present (see
`.env.example`): `WEBHOOK_URL`, or `JIRA_BASE_URL`/`JIRA_EMAIL`/
`JIRA_API_TOKEN`, or `GLPI_BASE_URL`/`GLPI_APP_TOKEN`/`GLPI_USER_TOKEN`. A
workflow whose `target_system` has no matching connector fails ticket
dispatch with a logged configuration error rather than panicking.

## API reference

The JSON API lives under `/api`, relative to `SERVER_BIND_ADDR` (Docker: the
host port from `.env`'s `APP_PORT`) -- `/health` and `/uploads/{*path}` are
the only exceptions, kept unprefixed for the Docker healthcheck and simple
asset URLs. Auth-protected routes take a `Bearer` access token; admin-only
routes additionally require `role = admin`. The web UI (see "Frontend"
below) is a separate set of page routes at the site root that authenticate
via a session cookie instead, and isn't listed here.

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| GET | `/health` | none | liveness check |
| GET | `/api/config` | none | org name, seal, `allow_local_signup`, `sso_enabled` |
| GET | `/uploads/{*path}` | none | serves uploaded org/department seals |
| POST | `/api/auth/login` | none | local email/password login |
| POST | `/api/auth/register` | none | local self-registration, 403 unless `allow_local_signup` |
| POST | `/api/auth/refresh` | none | rotate a refresh token for a new access/refresh pair |
| POST | `/api/auth/logout` | none | revoke a refresh token |
| POST | `/api/auth/password-reset/request` | none | email a reset link if `email` has a local-password account; 404 if no `Mailer` is configured |
| POST | `/api/auth/password-reset/confirm` | none | redeem a reset token for a new password, revoking existing sessions |
| GET | `/api/auth/sso/login` | none | redirects to Microsoft Entra ID |
| GET | `/api/auth/sso/callback` | none | Entra ID's redirect target; provisions/links the user |
| POST | `/api/auth/sso/token` | none | exchanges a one-time handoff code for a token pair |
| GET | `/api/users/me` | user | current user's profile |
| GET | `/api/users` | admin | list users |
| POST | `/api/users` | admin | create a user (local or pre-provisioned for SSO) |
| PATCH | `/api/users/{id}` | admin | update a user |
| POST | `/api/users/{id}/deactivate` | admin | deactivate a user and revoke their sessions |
| GET | `/api/departments` | user | list departments |
| POST | `/api/departments` | admin | create a department |
| PATCH | `/api/departments/{id}` | admin | update a department |
| DELETE | `/api/departments/{id}` | admin | delete a department |
| POST | `/api/departments/{id}/logo` | admin | upload a department seal (raw image bytes) |
| DELETE | `/api/departments/{id}/logo` | admin | remove a department seal |
| PATCH | `/api/admin/settings` | admin | update org name / `allow_local_signup` |
| POST | `/api/admin/settings/logo` | admin | upload the org-wide seal |
| DELETE | `/api/admin/settings/logo` | admin | remove the org-wide seal |
| GET/POST | `/api/catalog` | user/admin | list or create catalog items |
| PATCH | `/api/catalog/{id}` | admin | update a catalog item |
| POST | `/api/catalog/{id}/deactivate` | admin | deactivate a catalog item |
| GET/POST | `/api/workflow-definitions` | admin | list or create workflow definitions |
| POST | `/api/workflow-definitions/{id}/publish` | admin | publish a definition |
| POST | `/api/workflow-definitions/{id}/unpublish` | admin | unpublish a definition |
| GET/POST | `/api/instances` | user | list your instances, or start a new one |
| GET | `/api/instances/{id}` | user | fetch one instance (requester/approver/agent/admin only) |
| POST | `/api/instances/{id}/answers` | user | submit an answer, advancing the workflow |
| GET | `/api/approvals` | user | pending approvals assigned to you |
| POST | `/api/approvals/{id}/decide` | user | approve or reject |
| GET | `/api/audit-log` | admin | paginated audit trail (`?page=`, `?page_size=`, newest first) |

## Authentication

Two ways in, both ending in the same access/refresh JWT pair:

- **Local email/password** (`POST /api/auth/login`): always available for
  the bootstrap admin. Self-registration (`POST /api/auth/register`) is
  gated by `org_settings.allow_local_signup`, which defaults to `false` and
  is meant to stay off for orgs relying on SSO; `GET /api/config` exposes
  the current value. The web UI itself doesn't expose a self-registration
  page today (admins create accounts from `/admin/users` instead) -- this
  flag mainly matters if something else calls the JSON API directly.
  Self-service password reset (`POST /api/auth/password-reset/request`
  -> `.../confirm`, or the web UI's `/forgot-password` ->
  `/reset-password` pages) is available whenever `SMTP_HOST`/
  `SMTP_FROM_ADDRESS` are configured (see `.env.example` and "What's
  stubbed" below) -- a single-use, 30-minute token (`password_resets`,
  hashed the same way as `sessions.refresh_token_hash`) is emailed to the
  account, and redeeming it revokes all of that user's existing sessions.
- **Microsoft Entra ID (Azure AD) SSO** (`GET /api/auth/sso/login` ->
  `/api/auth/sso/callback` -> `POST /api/auth/sso/token`): this server is
  the confidential OAuth client (holds the client secret) and validates the
  ID token itself (signature via Entra ID's JWKS, issuer, audience, nonce)
  before provisioning a local user. See `crates/server/src/sso.rs` for the
  full flow, and `AppConfig::from_env` for the five `AZURE_*`/
  `SSO_FRONTEND_REDIRECT_URL` vars that enable it.

  A first-time sign-in matches or creates the local `User` row:
  `external_idp_subject` if this person has signed in before; otherwise by
  email, linking a password-less account an admin pre-created for them;
  otherwise auto-provisioning a new `requester` account. Auto-provisioning
  is safe here because Entra ID (via app assignment / conditional access)
  is what actually gates who can complete a login at all, a materially
  different trust boundary than the local self-registration path.

  The callback redirects the browser to `SSO_FRONTEND_REDIRECT_URL` with a
  short-lived, single-use `?code=...`. For the built-in web UI, point that
  var at this server's own `/login/sso/complete`, which redeems the code and
  sets the same session cookie a local login would (see "Frontend" below);
  `POST /api/auth/sso/token` is what a separate JS/Tauri client would call
  instead if one is ever built against the JSON API directly.

## Organization branding

`org_settings` (a singleton row) holds the org's display name and an
optional org-wide seal/logo; `departments.logo_url` optionally overrides it
per department. `GET /config` is public (no auth) so a not-yet-logged-in
frontend can render both before login; `PATCH /admin/settings`,
`POST`/`DELETE /admin/settings/logo` (org-wide), and
`POST`/`DELETE /departments/{id}/logo` (per-department) are admin-only.
Uploads are raw image bytes (PNG/JPEG/SVG/WebP, 5MB max) with `Content-Type`
set accordingly, not `multipart/form-data`, and are written under
`UPLOADS_DIR` (bind-mounted to a Docker volume so they survive a redeploy)
and served back from `/uploads/...`.

`APP_NAME`/`ALLOW_LOCAL_SIGNUP` env vars only seed `org_settings` the first
time the server starts against a fresh database; change them afterwards via
`PATCH /admin/settings`, not by editing `.env` and restarting.

`org_settings.timezone` (an IANA name, e.g. `America/Chicago`; defaults to
`UTC`) is admin-editable the same way, from `/admin/settings` or
`PATCH /admin/settings`, and controls only how timestamps are *displayed* --
in the web UI (requests/approvals lists) and in approval notification
emails. Every timestamp is still stored and passed between components in
UTC; changing this setting is always safe and has no effect on connector
payloads, the audit log, or anything else on-disk.

## Security & privacy notes

See [SECURITY.md](SECURITY.md) for the full list and the production
hardening checklist. Highlights:

- Passwords are hashed with Argon2id; plaintext never reaches the db crate.
  Accounts can alternatively be SSO-only (`external_idp_subject`, no
  password) via Microsoft Entra ID; see "Authentication" above.
- Access tokens are short-lived JWTs (default 15 min); refresh tokens are
  opaque, single-use, and stored only as a SHA-256 hash in `sessions`. A
  leaked database dump doesn't hand out working refresh tokens, and
  deactivating a user (or a manual logout) revokes them immediately rather
  than waiting out a TTL.
- Every request re-checks `is_active` against the database, so a deactivated
  account's still-unexpired access token stops working immediately.
- All queries are parameterized through sqlx; no string-built SQL.
- `audit_log` is an append-only trail, readable at `/admin/audit-log`
  (or `GET /api/audit-log`) for after-the-fact review. Only user
  create/update/deactivate is actually recorded today -- catalog/workflow
  changes and approval decisions aren't audited yet despite being the
  kind of thing this table exists for; adding those is just more call
  sites to `audit::record`, not a schema change.
- CORS defaults to allowing no cross-origin browser access at all unless
  `CORS_ALLOWED_ORIGINS` is set: safer than an accidental wildcard.
- `WorkflowInstance.answers_json` may contain requester-submitted personal
  data; only the requester, resolved approvers, agents, and admins can read
  an instance (enforced in `routes/instances.rs`). Apply your organization's
  retention policy to `workflow_instances`/`audit_log` independently; this
  starter does not purge old data.
- The generic `WebhookConnector` posts to an admin-configured URL; if
  workflow authoring is ever opened up to less-trusted admins, put an
  egress allowlist in front of it to avoid SSRF against internal-only
  services.

## What's stubbed / left for follow-up

- **Notifications**: `ApprovalNotifier` emails the approver via SMTP
  (`SmtpNotifier`, backed by the shared `Mailer`) when `SMTP_HOST`/
  `SMTP_FROM_ADDRESS` are configured (see `.env.example`); otherwise it
  falls back to `LoggingNotifier`, which only writes to the server log.
  Slack/Teams are still unwired -- nothing in the approval flow depends on
  notification actually succeeding either way. The same `Mailer` also backs
  self-service password reset (see "Authentication" above), so both
  features come online together once SMTP is configured.
- **Approver fan-out**: `ApproverResolution::RoleInDepartment` picks the
  first matching active user rather than creating a multi-approver queue.
- **Ticket dispatch retries**: a failed dispatch marks the ticket `failed`
  and the instance `failed`; there's no automatic retry job yet (see
  `tickets_repo::list_pending_dispatch`, which a retry worker would poll).

## Frontend

The web UI is server-rendered with [Askama](https://docs.rs/askama)
(compile-time-checked HTML templates, `crates/server/templates/`) and lives
in `crates/server/src/web/`, mounted at the site root alongside the JSON API
(nested under `/api`, see "API reference" above). It authenticates browsers
via an httponly session cookie bridged to the same JWT/refresh-token
machinery the JSON API uses (`crates/server/src/web/session.rs`) rather than
a bearer header, so there's no separate frontend deployment, build step, or
Node toolchain -- `cargo build` produces one binary that serves both.

Pages, by role:

- **Everyone**: `/` dashboard, `/catalog` (browse + start a request),
  `/requests` and `/requests/{id}` (the request wizard -- each
  `WorkflowDefinition::Question` step renders as one form, submitted one
  answer at a time, matching how `workflow-engine` actually advances an
  instance), `/approvals` (decide anything resolved to you, regardless of
  role -- same as the JSON API).
- **Admin** (`/admin/users`, `/admin/departments`, `/admin/catalog`,
  `/admin/workflows`, `/admin/settings`, `/admin/audit-log`): user/
  department/catalog management, org branding/signup toggle, workflow
  definitions, and a paginated, read-only view of `audit_log`.
  `/admin/workflows`'s "Create a workflow definition" section is a
  drag-and-drop graph builder (`crates/server/static/workflow_builder.js`,
  vanilla JS, no build step, matching the rest of this project) --
  add/wire/drag `Question`/`Branch`/`WaitForApproval`/`SubmitTicket`/`End`
  step nodes and it stays in sync with the same `graph_json` the form
  posts. Nested AND/OR/NOT branch conditions aren't editable visually
  (round-trip preserved, just not buildable from scratch in the UI) --
  use "Advanced: edit as JSON" under the builder for those. Field mapping
  has no visual editor at all, only that same JSON textarea.

Since AD-group-to-role mapping isn't implemented, a user's `role` (and
therefore which admin pages they can reach) is a plain column set by another
admin or, for a first SSO login, `Requester` by default; nothing here reads
Entra ID group claims yet.

Tauri packaging is still on the table for a native desktop shell: since this
is an ordinary server-rendered site (no SPA routing tricks, no client-side
state that assumes a `file://` origin), a Tauri shell can simply point its
webview at this server's URL the same way a browser would, rather than
bundling built JS assets the way an SPA-based Tauri app would.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for dev environment setup, coding
conventions, and what a pull request is expected to include.

## License

GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later). See
[LICENSE](LICENSE). In short: if you run a modified version of this to
serve users over a network, you must make that modified source available
to those users, not just to people you distribute a binary to.
