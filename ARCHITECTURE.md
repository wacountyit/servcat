# Architecture

This document describes how ServCat's backend is put together: the crate
map, the data model, the request lifecycle for a service request, the
authentication design, and the Docker deployment topology. For setup
instructions see [README.md](README.md); for the production hardening
checklist see [SECURITY.md](SECURITY.md).

## Goals

- Be a thin, configurable front end for an existing ticketing system
  (Jira, GLPI, Freshservice, or a generic webhook), not a ticketing system
  in its own right.
- Let an admin change what a catalog item asks, who approves it, and where
  it's filed, without a code change or redeploy: workflows and field
  mappings are data (JSON in the database), not code.
- Keep side effects (database writes, approver notification, ticket
  dispatch) in one place (`server::orchestrator`) so the actual decision
  logic (`workflow-engine`) stays a pure, trivially-testable function of
  "current state + an answer" to "next state."

## Crate map

```
crates/
|-- model/             Shared domain types (User, WorkflowDefinition, ...)
|-- db/                sqlx/MariaDB pool + migrations + repositories
|-- workflow-engine/   Pure, synchronous graph interpreter (no I/O)
|-- connectors/        TicketConnector trait + Jira/GLPI/Webhook + field-mapping templates
|-- approvals/         Approver resolution, notification, expiry polling
`-- server/            Axum API: auth, routes, and the orchestrator that
                        wires the engine to connectors/approvals/db
```

- **`model`**: plain data types shared by every other crate (`User`,
  `Department`, `WorkflowDefinition`, `WorkflowInstance`, `Ticket`,
  `PendingApproval`, `OrgSettings`, ...), plus their `sqlx::FromRow` impls.
  No I/O, no business logic. This is what keeps the on-disk (JSON/SQL)
  shape and the in-memory shape from drifting apart across crates.
- **`db`**: one repository module per aggregate
  (`repositories::{users,departments,catalog,workflow_definitions,
  instances,approvals,tickets,audit,org_settings}`), each just a thin,
  parameterized-SQL wrapper around `sqlx`. `connect_and_migrate` opens the
  pool and runs `sqlx::migrate!("../../migrations")`, which embeds the
  migration SQL into the compiled binary at build time (nothing is read
  from disk at runtime for migrations).
- **`workflow-engine`**: interprets a `WorkflowDefinition` (a graph of
  `Step`s: `Question`, `Branch`, `WaitForApproval`, `SubmitTicket`, `End`)
  against a `WorkflowInstance`'s current state and a new answer, and
  returns an `Outcome` (`AwaitingAnswer`, `AwaitingApproval`,
  `ReadyToSubmitTicket`, `Finished`). It performs no I/O and knows nothing
  about the database, HTTP, or connectors, which is what makes
  `crates/workflow-engine/src/lib.rs`'s tests run with no setup at all.
- **`connectors`**: the `TicketConnector` trait plus one implementation per
  target system (`jira.rs`, `glpi.rs`, `webhook.rs`) and the field-mapping
  renderer (`field_mapping.rs`), which renders each target field's Tera
  template against the collected answers, the requester's profile, and
  instance metadata. `Freshservice` is a valid `target_system` value in the
  schema with no connector implemented yet; dispatch to it fails with a
  logged configuration error.
- **`approvals`**: resolves an `ApproverResolution` (`Static`,
  `ManagerOfRequester`, `RoleInDepartment`) to a concrete user id, creates
  the `PendingApproval` row, fires a (currently log-only) notification, and
  polls for expired approvals on a timer (`spawn_expiry_poller`).
- **`server`**: the Axum application. `routes/` holds the JSON API, one
  module per resource, nested under `/api` in `routes::build_router`;
  `auth.rs` and `sso.rs` (top-level, not under `routes/`) hold the actual
  token/session/SSO logic that both `routes/*` and `web/*` handlers call
  into; `orchestrator.rs` is the one place that wires the workflow engine's
  pure `Outcome` to real database writes, approver resolution, and connector
  dispatch. `web/` is the server-rendered web UI (Askama templates under
  `templates/`) mounted at the site root alongside `/api` -- see
  "Frontend" in README.md for the page list and `web/session.rs` for how it
  turns the JSON API's bearer-JWT auth into a browser session cookie.

## Data model

All tables live in `migrations/`, applied in order by `sqlx migrate`:

| Migration | Tables | Notes |
| --- | --- | --- |
| `0001_departments.sql` | `departments` | self-referencing `parent_department_id` for a department hierarchy |
| `0002_users.sql` | `users`, `sessions` | a user is local (`password_hash`), SSO (`external_idp_subject`), or both; `sessions` stores only a SHA-256 hash of each refresh token |
| `0003_workflow_definitions.sql` | `workflow_definitions`, `service_catalog_items` | `definition_json`/`field_mapping_json` are the actual workflow graph and per-target-system field mapping |
| `0004_workflow_instances.sql` | `workflow_instances`, `pending_approvals`, `tickets` | one row per in-flight or completed request, its approval(s), and its dispatched ticket |
| `0005_audit_log.sql` | `audit_log` | append-only; never updated or deleted by the application |
| `0006_org_settings.sql` | `org_settings` | singleton (`id = 1`), org display name, seal, `allow_local_signup` |
| `0007_department_logo.sql` | (alters `departments`) | adds `logo_url`, an optional per-department seal override |

`Id = uuid::Uuid` and `Timestamp = chrono::DateTime<Utc>` (see
`crates/model/src/lib.rs`) everywhere. IDs are stored as `BINARY(16)`, not a
string, to keep indexes small.

## Request lifecycle: submitting a service request

1. `POST /api/instances` starts a `WorkflowInstance` against a published
   `WorkflowDefinition`, at that definition's entry step.
2. `POST /api/instances/{id}/answers` hands one answer to
   `workflow-engine::advance`, which validates it against the current
   step and returns the next `Outcome`.
3. `server::orchestrator` acts on that `Outcome`:
   - `AwaitingAnswer`: just persists the new `current_step_id` and merged
     `answers_json`.
   - `AwaitingApproval`: calls into `approvals::create_for_instance`, which
     resolves the approver, writes `pending_approvals`, and notifies.
   - `ReadyToSubmitTicket`: renders the definition's `field_mapping`
     templates against the collected answers and dispatches to the
     matching `TicketConnector`, writing the result to `tickets`.
   - `Finished`: marks the instance `completed`/`rejected`/`cancelled` and
     sets `completed_at`.
4. An approval decision (`POST /api/approvals/{id}/decide`) or an expired
   approval (the background poller in `approvals::spawn_expiry_poller`)
   re-enters the same `workflow-engine::advance` path with a synthetic
   answer, so approval handling and question-answering share one code path
   rather than being a special case.

## Authentication architecture

Both paths end at the same place: `auth::issue_token_pair`, which mints a
short-lived JWT access token and an opaque, single-use refresh token
(stored only as a SHA-256 hash in `sessions`).

```
Local:  POST /api/auth/login  ---------------------------> token pair

SSO:    GET /api/auth/sso/login
          -> redirect to Entra ID (PKCE + nonce recorded in-memory)
        GET /api/auth/sso/callback  (Entra ID redirects back with ?code&state)
          -> exchange code for an ID token, validate it (JWKS signature,
             issuer, audience, nonce)
          -> match/create the local User row
          -> redirect to SSO_FRONTEND_REDIRECT_URL?code=<one-time code>
        POST /api/auth/sso/token  { code }  ---------------> token pair
```

`crates/server/src/sso.rs` implements the OAuth2 authorization-code + PKCE
flow and ID token validation directly against `reqwest` and `jsonwebtoken`
rather than pulling in a dedicated OAuth/OIDC crate, to keep the dependency
footprint small and keep full control over what gets validated (issuer,
audience, nonce, signature) and how identities get matched to local users.

**Known constraint**: the PKCE/nonce pending-login map and the one-time
handoff-code map in `SsoService` are in-process (`std::sync::Mutex` over a
`HashMap`), not shared storage. That's fine for a single `app` replica
(the only topology `docker-compose.yml` runs today) but means an in-flight
SSO login won't survive a redeploy mid-flow (the user just retries), and it
would need to move to shared storage (Redis, or a table with a TTL sweep)
before running more than one `app` replica behind a load balancer.

User matching on a successful SSO login, in order:

1. `external_idp_subject` matches an existing user: that's them.
2. No subject match, but the asserted email matches an existing user with
   no `external_idp_subject` yet: link it (this is how an admin
   pre-provisions an approver/admin account by email ahead of that
   person's first SSO login).
3. Neither matches: auto-provision a new `requester`. This is safe because
   Entra ID (via app assignment or conditional access on the tenant) is
   what actually gates who can complete a login at all, a materially
   different trust boundary than local self-registration, which is why the
   two aren't governed by the same toggle.

## Uploads

Org/department seal uploads (`crates/server/src/uploads.rs`) take the
image as a raw request body with `Content-Type` set accordingly (not
`multipart/form-data`), validate it against an allowlist (PNG, JPEG, SVG,
WebP) and a 5MB cap, and write it under `UPLOADS_DIR` with a random
filename per upload (so a replacement doesn't collide with a cached copy
of the old file at the same URL). `routes/uploads.rs` serves them back with
a small hand-rolled handler rather than `tower_http::services::ServeDir`,
since this only ever needs to serve the fixed, small set of image files
this server itself wrote.

## Deployment topology (Docker)

```
                        +-------------------+
   :80/:443 (optional)  |  caddy             |
   ------------------->  |  (profile: caddy)  |
                        +---------+----------+
                                  |
                                  v
                        +-------------------+        +-------------------+
   host:APP_PORT ------> |  app               | -----> |  mariadb           |
   (default 8080)       |  servcat-server    |        |  servcat-db        |
                        +-------------------+        +-------------------+
                          servcat_uploads volume        servcat_db_data volume
                                                         127.0.0.1:3306 published
                                                         (host tooling only)
```

- `mariadb`'s port is published to `127.0.0.1` only, so `scripts/migrate.sh`
  (sqlx-cli, run on the host) and `scripts/backup-db.sh` can reach it
  without exposing the database to the network.
- `app`'s healthcheck hits `GET /health`; `mariadb`'s healthcheck requires
  `--innodb_initialized`, with a 120-second `start_period` because a brand
  new data volume's first-time initialization can take well over a minute,
  longer than the default retry budget would tolerate.
- Re-running `docker compose up -d --build` after a code change only
  rebuilds and recreates `app`; `mariadb` (and its data) are untouched,
  which is what gives a redeploy its low downtime.
- `caddy` only runs under the `caddy` Compose profile (`install.sh` sets
  `COMPOSE_PROFILES=caddy` in `.env` if you ask it to manage TLS for you).

## Design decisions worth knowing about

- **Bearer JWTs, not cookies.** The API is consumed by both a browser SPA
  and a Tauri desktop app (a different origin/custom scheme), where a
  cookie-based session doesn't travel cleanly. A refresh token is opaque
  and single-use specifically so a leaked value is only good once and a
  logout/deactivation can revoke it immediately, unlike a stateless JWT
  that's only bounded by its (short) TTL.
- **`org_settings` is a real table, not env vars**, even though `APP_NAME`
  and `ALLOW_LOCAL_SIGNUP` env vars exist. The env vars only seed the row
  once, on first startup against a fresh database; an admin changes them
  afterwards through the API without a redeploy.
- **Workflow definitions and field mappings are JSON in the database, not
  Rust types matching each catalog item.** Reconfiguring what a catalog
  item asks, or which Jira project/GLPI entity type it targets, is an
  admin action, not a code change.
