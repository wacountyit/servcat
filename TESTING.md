# Testing

## Automated tests

`cargo test --workspace` runs today without a database:

- `crates/workflow-engine/src/lib.rs`: the graph interpreter's tests
  (entry step, branch true/false, cycle detection, rejecting an answer for
  the wrong field).
- `crates/connectors/src/field_mapping.rs`: the Tera-template field-mapping
  renderer's tests.

That's the full extent of automated coverage right now. Everything under
`crates/server` and `crates/db` (routes, auth, SSO, uploads, repositories)
has no automated tests yet: they need a real MariaDB instance and are
exercised manually today. If you add integration tests against a real
database, this section should grow to describe how to point them at one
(a Docker Compose test service, `sqlx::test`, etc.).

Run `cargo check --workspace` before every change; it's fast and catches
most mistakes without needing a database at all.

## Manual QA checklist

Work through this after any change touching auth, uploads, org settings,
or the Docker/install tooling. Use `curl` or Postman against the running
API; there is no frontend in this repo yet to click through.

### Deployment

- [ ] `./install.sh` completes without errors on a machine with nothing
      set up yet, and `docker compose ps` shows `servcat-db` and
      `servcat-app` both healthy.
- [ ] `curl http://localhost:$APP_PORT/health` returns `{"status":"ok"}`.
- [ ] `curl http://localhost:$APP_PORT/config` returns the seeded
      `app_name` and `allow_local_signup` from `.env`.
- [ ] Change a source file, run `docker compose up -d --build`, and
      confirm (`docker compose ps`) that `servcat-db`'s uptime is
      undisturbed while `servcat-app` recreates. This is the "minimal
      downtime redeploy" the whole Docker setup exists for.
- [ ] `scripts/migrate.sh info` reports all migrations applied; run it
      against a fresh database too, to confirm they apply cleanly in
      order from empty.
- [ ] `scripts/backup-db.sh` produces a `.sql.xz` file, and
      `scripts/restore-db.sh` against a scratch database actually restores
      it. Don't wait until a real incident to find out the restore path is
      broken.
- [ ] From a second machine on the same network, confirm the app is
      reachable on the host's real IP and port (not just `localhost`); if
      not, check the host firewall (see the port-forwarding note in
      README.md/ARCHITECTURE.md).

### Local authentication

- [ ] Logging in as the bootstrap admin (`POST /auth/login`) returns an
      access token, a refresh token, and the expected `user` profile.
- [ ] `POST /auth/register` returns 403 while `allow_local_signup` is
      `false` (the default).
- [ ] After `PATCH /admin/settings {"allow_local_signup": true}` as an
      admin, `POST /auth/register` succeeds with a new email, and fails
      with 409 on a duplicate email.
- [ ] `POST /auth/register` rejects a password under 12 characters.
- [ ] `POST /auth/refresh` with a used-up (already-redeemed) refresh token
      fails; refresh tokens are single-use.
- [ ] `POST /auth/logout` followed by another `POST /auth/refresh` with
      the same token fails.
- [ ] Deactivate a user (`POST /users/{id}/deactivate`) while they hold a
      still-unexpired access token, then confirm their next request with
      that token gets 401. `is_active` is re-checked on every request, so
      this should be immediate, not wait out the token's TTL.

### SSO (once Microsoft Entra ID is configured)

- [ ] `GET /auth/sso/login` redirects to
      `login.microsoftonline.com/<tenant>/oauth2/v2.0/authorize` with your
      client id and a `code_challenge`.
- [ ] Completing a real Entra ID login redirects back to
      `SSO_FRONTEND_REDIRECT_URL?code=...`, and `POST /auth/sso/token`
      with that code returns a token pair.
- [ ] A second `POST /auth/sso/token` with the same code fails (single-use).
- [ ] A brand-new Entra ID identity (no existing user by subject or email)
      auto-provisions a `requester`.
- [ ] An admin-created user with just an email (no password, no
      `external_idp_subject`) gets linked, not duplicated, on that
      person's first SSO login with the matching email.
- [ ] An Entra ID login whose email matches a user already linked to a
      *different* subject is rejected (409), not silently reassigned.
- [ ] Without `AZURE_TENANT_ID`/etc. set at all, `/auth/sso/*` routes
      return 404 rather than panicking, and `GET /config` reports
      `sso_enabled: false`.

### Organization branding

- [ ] `GET /config` works with no `Authorization` header at all.
- [ ] `PATCH /admin/settings` as a non-admin returns 403.
- [ ] `POST /admin/settings/logo` with a PNG under 5MB succeeds and
      updates `GET /config`'s `logo_url`; the previous file (if any) is
      gone from `UPLOADS_DIR`, not just orphaned.
- [ ] The same upload with an unsupported `Content-Type` (e.g. `text/plain`)
      or a file over 5MB is rejected.
- [ ] `DELETE /admin/settings/logo` clears `logo_url` and removes the file.
- [ ] Repeat the last three checks against
      `POST`/`DELETE /departments/{id}/logo` for a department seal.

### Workflow lifecycle

- [ ] Create and publish a `WorkflowDefinition` with at least one branch
      and one approval step, and a catalog item pointing at it.
- [ ] `POST /instances` then `POST /instances/{id}/answers` walks the
      instance through `AwaitingAnswer` -> `AwaitingApproval` and creates a
      `pending_approvals` row for the expected approver.
- [ ] `POST /approvals/{id}/decide` as someone who is *not* the resolved
      approver returns 403.
- [ ] Approving advances the instance to ticket dispatch (check the
      `tickets` row and, for the webhook connector, that the configured
      URL actually received the rendered payload); rejecting ends the
      instance as `rejected` without dispatching anything.
- [ ] An expired approval (set a short `timeout_seconds` in the workflow
      definition to test this quickly) gets picked up by the background
      poller within its polling interval and the instance reflects that
      without any manual intervention.
- [ ] A workflow whose `target_system` has no configured connector fails
      dispatch with a logged error and a `failed` ticket/instance status,
      not a crash.
