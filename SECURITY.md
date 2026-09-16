# Security policy

## Reporting a vulnerability

This is an internal government IT tool, not a public bug bounty program.
If you find a security issue:

- Do not open a public issue or pull request describing the vulnerability
  or including exploit details.
- Report it privately to the repository maintainer / your IT security
  team, with enough detail to reproduce it (affected endpoint or file,
  steps, and impact).
- Give a reasonable amount of time to fix it before discussing it anywhere
  more widely, especially if this deployment is reachable outside your own
  network.

_(Fill in an actual contact, e.g. a security distribution list or the
maintainer's work email, before relying on this section.)_

## Supported versions

Pre-1.0: only the `main` branch is supported. There are no maintained
release branches yet; see [CHANGELOG.md](CHANGELOG.md) for what's shipped
so far.

## Security posture

This section summarizes what the codebase actually does today. For the
architecture behind these decisions, see [ARCHITECTURE.md](ARCHITECTURE.md).

**Authentication**

- Local passwords are hashed with Argon2id (`crates/server/src/auth.rs`);
  plaintext never reaches the `db` crate.
- Access tokens are short-lived JWTs (`JWT_ACCESS_TOKEN_TTL_SECONDS`,
  default 900s). Refresh tokens are opaque, single-use (rotated on every
  use), and stored only as a SHA-256 hash in `sessions`, so a leaked
  database dump alone does not hand out working refresh tokens.
- Every request re-checks `is_active` against the database, so
  deactivating a user (or a manual logout revoking a refresh token) takes
  effect immediately rather than waiting out a token's TTL.
- SSO (Microsoft Entra ID) validates the ID token's signature against
  Entra ID's live JWKS, its issuer and audience, and a per-login-attempt
  nonce, before trusting any claim in it. The OAuth exchange uses PKCE in
  addition to a confidential client secret.
- Local self-registration (`POST /auth/register`) is off by default
  (`org_settings.allow_local_signup = false`) and independently enforced
  server-side, not just hidden in a frontend.

**Data handling**

- All database queries are parameterized through sqlx; there is no
  string-built SQL anywhere in the codebase.
- `WorkflowInstance.answers_json` may contain requester-submitted personal
  data. Access is restricted to the requester, the resolved approver(s),
  agents, and admins (`routes/instances.rs`). This project does not
  automatically purge or retire old data; apply your organization's
  records-retention policy to `workflow_instances` and `audit_log`
  yourselves.
- `audit_log` is append-only at the application layer (nothing updates or
  deletes rows in it) for after-the-fact review of admin and approval
  actions. It has no additional tamper-evidence (e.g. hash chaining); treat
  database-level access controls and backups as your actual protection
  against a compromised admin account editing history directly.

**Network surface**

- CORS defaults to allowing no cross-origin browser access at all unless
  `CORS_ALLOWED_ORIGINS` is explicitly set; there is no wildcard fallback.
- The generic `WebhookConnector` posts to an admin-configured URL with no
  egress allowlist. If workflow authoring is ever opened up to
  less-trusted admins, that connector can be pointed at internal-only
  services (SSRF); put an allowlist in front of it first.
- Uploaded org/department seals are validated against a fixed content-type
  allowlist (PNG, JPEG, SVG, WebP) and a 5MB size cap
  (`crates/server/src/uploads.rs`), and are served back by a small
  handler that rejects `..`/empty path segments
  (`crates/server/src/routes/uploads.rs`) rather than a general-purpose
  static file server.

**Known gaps (not yet built)**

- No rate limiting on `POST /auth/login` or `POST /auth/register`. Put a
  reverse proxy or WAF rate limit in front of both if this is reachable
  from an untrusted network.
- `ApprovalNotifier` only logs; nothing in the approval flow depends on a
  notification actually reaching anyone, but there is also no email/Slack
  trail to audit approver awareness against.
- Ticket dispatch has no automatic retry on failure; a failed dispatch
  needs manual follow-up today (see `tickets_repo::list_pending_dispatch`).
- The SSO pending-login and one-time handoff-code state is in-process
  memory, not shared storage. This is fine for the single-`app`-replica
  topology this repo ships (`docker-compose.yml`), but would need to move
  to shared storage before running multiple replicas behind a load
  balancer; see "Known constraint" in ARCHITECTURE.md.

## Production hardening checklist

Work through this before exposing a deployment beyond your own machine:

- [ ] Rotate or disable the bootstrap admin account once real admin
      accounts (local or SSO) exist.
- [ ] Terminate TLS in front of this app (Caddy via `install.sh`, or your
      own reverse proxy) before it's reachable outside `localhost`.
      Required for Entra ID's redirect URI regardless.
- [ ] Set `CORS_ALLOWED_ORIGINS` to your actual frontend origin(s) only,
      never a wildcard.
- [ ] Let `install.sh` generate `JWT_SIGNING_SECRET` and the database
      passwords; never reuse the placeholder values from `.env.example`.
- [ ] Configure Microsoft Entra ID SSO and keep `ALLOW_LOCAL_SIGNUP=false`
      unless you specifically intend to allow public self-registration.
- [ ] Keep MariaDB's published port bound to `127.0.0.1` (the default in
      `docker-compose.yml`) or remove the port mapping entirely if you
      don't need host-side `sqlx-cli` access.
- [ ] If the host is on a shared or multi-tenant network, firewall
      everything except the reverse proxy's port; don't rely on the app
      port alone being "not commonly guessed."
- [ ] Put `scripts/backup-db.sh` on a schedule (cron/systemd timer) and
      actually run `scripts/restore-db.sh` against a test database at
      least once before you need it for real.
- [ ] Review "What's stubbed / left for follow-up" in
      [README.md](README.md) (notifications, approver fan-out, ticket
      retry) before depending on any of it operationally.
