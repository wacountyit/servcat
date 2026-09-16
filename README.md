# ServCat

A self-hosted IT service catalog / self-service portal, in the spirit of
ServiceNow's Employee Self-Service Catalog or GLPI's self-service module -
lightweight, org-customizable, and designed to front an existing ticketing
system (Jira, GLPI, Freshservice, or a generic webhook) rather than reinvent
one.

This repo is the Rust backend: a workflow interpreter, a MariaDB-backed
domain model, and an Axum API. A TypeScript/React frontend (web + Tauri
desktop, one codebase) consumes this API, planned as a separate repo; see
"Frontend" below for why.

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
account, the local sign-up toggle, and (optionally) Microsoft Entra ID SSO,
then runs `docker compose up -d --build`. Re-running
`docker compose up -d --build` after a code change only rebuilds/recreates
the `app` container; MariaDB keeps running, so there's no database downtime
on a redeploy.

See `scripts/migrate.sh` (sqlx-cli, run from the host against the
Docker-exposed `127.0.0.1:3306`), `scripts/backup-db.sh`, and
`scripts/restore-db.sh` for day-to-day operations.

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

All routes are relative to `SERVER_BIND_ADDR` (Docker: the host port from
`.env`'s `APP_PORT`). Auth-protected routes take a `Bearer` access token;
admin-only routes additionally require `role = admin`.

| Method | Path | Auth | Purpose |
| --- | --- | --- | --- |
| GET | `/health` | none | liveness check |
| GET | `/config` | none | org name, seal, `allow_local_signup`, `sso_enabled` |
| GET | `/uploads/{*path}` | none | serves uploaded org/department seals |
| POST | `/auth/login` | none | local email/password login |
| POST | `/auth/register` | none | local self-registration, 403 unless `allow_local_signup` |
| POST | `/auth/refresh` | none | rotate a refresh token for a new access/refresh pair |
| POST | `/auth/logout` | none | revoke a refresh token |
| GET | `/auth/sso/login` | none | redirects to Microsoft Entra ID |
| GET | `/auth/sso/callback` | none | Entra ID's redirect target; provisions/links the user |
| POST | `/auth/sso/token` | none | exchanges a one-time handoff code for a token pair |
| GET | `/users/me` | user | current user's profile |
| GET | `/users` | admin | list users |
| POST | `/users` | admin | create a user (local or pre-provisioned for SSO) |
| PATCH | `/users/{id}` | admin | update a user |
| POST | `/users/{id}/deactivate` | admin | deactivate a user and revoke their sessions |
| GET | `/departments` | user | list departments |
| POST | `/departments` | admin | create a department |
| PATCH | `/departments/{id}` | admin | update a department |
| DELETE | `/departments/{id}` | admin | delete a department |
| POST | `/departments/{id}/logo` | admin | upload a department seal (raw image bytes) |
| DELETE | `/departments/{id}/logo` | admin | remove a department seal |
| PATCH | `/admin/settings` | admin | update org name / `allow_local_signup` |
| POST | `/admin/settings/logo` | admin | upload the org-wide seal |
| DELETE | `/admin/settings/logo` | admin | remove the org-wide seal |
| GET/POST | `/catalog` | user/admin | list or create catalog items |
| PATCH | `/catalog/{id}` | admin | update a catalog item |
| POST | `/catalog/{id}/deactivate` | admin | deactivate a catalog item |
| GET/POST | `/workflow-definitions` | admin | list or create workflow definitions |
| POST | `/workflow-definitions/{id}/publish` | admin | publish a definition |
| POST | `/workflow-definitions/{id}/unpublish` | admin | unpublish a definition |
| GET/POST | `/instances` | user | list your instances, or start a new one |
| GET | `/instances/{id}` | user | fetch one instance (requester/approver/agent/admin only) |
| POST | `/instances/{id}/answers` | user | submit an answer, advancing the workflow |
| GET | `/approvals` | user | pending approvals assigned to you |
| POST | `/approvals/{id}/decide` | user | approve or reject |

## Authentication

Two ways in, both ending in the same access/refresh JWT pair:

- **Local email/password** (`POST /auth/login`): always available for the
  bootstrap admin. Self-registration (`POST /auth/register`) is gated by
  `org_settings.allow_local_signup`, which defaults to `false` and is meant
  to stay off (and hidden in the frontend) for orgs relying on SSO;
  `GET /config` exposes the current value so the frontend knows whether to
  show a sign-up option at all.
- **Microsoft Entra ID (Azure AD) SSO** (`GET /auth/sso/login` ->
  `/auth/sso/callback` -> `POST /auth/sso/token`): this server is the
  confidential OAuth client (holds the client secret) and validates the ID
  token itself (signature via Entra ID's JWKS, issuer, audience, nonce)
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
  short-lived, single-use `?code=...`, which the frontend immediately
  exchanges via `POST /auth/sso/token` for a token pair. This keeps both
  Entra ID's tokens and ours out of the browser's address bar/history, and
  the exchange step is identical for the web app and a Tauri custom-scheme
  redirect.

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
- `audit_log` is an append-only trail of admin and approval actions
  (catalog/workflow changes, user/role changes, approval decisions) for
  after-the-fact review.
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

- **Notifications**: `ApprovalNotifier` only logs today. Wire it to real
  email/Slack/Teams before relying on it; nothing in the approval flow
  depends on notification actually succeeding.
- **Approver fan-out**: `ApproverResolution::RoleInDepartment` picks the
  first matching active user rather than creating a multi-approver queue.
- **Ticket dispatch retries**: a failed dispatch marks the ticket `failed`
  and the instance `failed`; there's no automatic retry job yet (see
  `tickets_repo::list_pending_dispatch`, which a retry worker would poll).

## Frontend

The frontend (not in this repo) is planned as TypeScript/React rather than
Leptos: this app is fundamentally a dynamic, conditionally-branching form
renderer today and a visual workflow graph builder for admins tomorrow, and
that's exactly where the JS ecosystem (JSON-Schema-driven forms,
react-hook-form, React Flow for the graph editor) is more mature than Rust's.
Tauri just points its webview at the built static assets, so the same
frontend ships as both a web app and a desktop app without giving up
"Rust for everything that matters" in this backend.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for dev environment setup, coding
conventions, and what a pull request is expected to include.

## License

GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later). See
[LICENSE](LICENSE). In short: if you run a modified version of this to
serve users over a network, you must make that modified source available
to those users, not just to people you distribute a binary to.
