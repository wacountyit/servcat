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
- [ ] On a machine that already has something listening on 8080 and/or
      3306 (e.g. `python3 -m http.server 8080` and a local MySQL/MariaDB),
      `install.sh` detects both, offers the next free port for each, and
      the resulting `.env`'s `DATABASE_URL` uses the chosen `DB_PORT` (not
      3306) -- `scripts/migrate.sh` should still connect successfully.
      Re-running `install.sh` against an `.env` from before `DB_PORT`
      existed backfills it and fixes up `DATABASE_URL` to match, without
      re-prompting for anything already recorded.
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
- [ ] With SMTP configured, `POST /auth/password-reset/request` for a real
      local account emails a working `/reset-password?token=...` link;
      for a nonexistent email, an SSO-only account, or a deactivated
      account, it responds identically (202) but sends nothing.
- [ ] `POST /auth/password-reset/confirm` with that token sets the new
      password (confirm you can log in with it), and a second confirm
      with the same token fails (single-use). An old refresh token from
      before the reset should no longer work afterwards.
- [ ] With SMTP unset, `POST /auth/password-reset/request` returns 404,
      and the web UI hides the "Forgot your password?" link and redirects
      `/forgot-password` to `/login`.

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
- [ ] `PATCH /admin/settings {"timezone": "America/Chicago"}` succeeds and
      the requests/approvals lists in the web UI immediately show times
      shifted accordingly (with a `CST`/`CDT` suffix); a bogus value like
      `"Not/AZone"` is rejected with 400 both from the JSON API and from
      `/admin/settings`'s form (re-rendered with an inline error, not a
      generic error page). A non-admin gets 403.

### Audit log

- [ ] Creating, updating, and deactivating a user each add a row to
      `audit_log`, visible newest-first at `/admin/audit-log` and via
      `GET /api/audit-log`; the actor shown is the admin who did it, not
      the affected user.
- [ ] `/admin/audit-log` and `GET /api/audit-log` both return 403 for a
      non-admin.
- [ ] With more than one page of entries, `?page=2` shows the next-older
      page and the "Newer"/"Older" links land on the expected pages; an
      out-of-range page (e.g. `?page=9999`) shows an empty table rather
      than erroring.
- [ ] `GET /api/audit-log?page_size=99999` is clamped to the server's max
      page size rather than returning the whole table in one response.

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

### Workflow graph builder (`/admin/workflows`)

Automated: `18` unit-style assertions against the builder's pure
serialization/hydration/validation logic pass in a headless JS engine
(exercised while building this feature; not wired into CI). No browser
was available in that session, so **the DOM-interaction parts below
(dragging, live re-rendering, focus behavior) still need a real
browser** -- please actually click through this section once.

- [ ] Add one of each step kind (`+ Question`/`+ Branch`/`+ Approval`/
      `+ Submit ticket`/`+ End`); each renders as a distinctly colored
      node, and dragging a node's header moves it and its arrows live.
- [ ] Wire `next`/`on_true`/`on_false`/`on_approve`/`on_reject` dropdowns
      between steps and confirm arrows are drawn correctly, with
      true/false and approve/reject labeled in green/red.
- [ ] Click the star on a step to mark it the entry step; only one step
      is ever marked at a time.
- [ ] Delete a step that something else still points to -- confirm you
      get a warning naming the referencing step(s) before it's removed.
- [ ] For a `Question` step, switching "Input type" to `select`/
      `multi_select` reveals an options editor (add/remove rows); other
      input types hide it.
- [ ] For a `Branch` step, build an `equals`/`not_equals`/`in` condition
      of each value type (text/number/true-false) and confirm "Advanced:
      edit as JSON" shows the expected shape (`{"op": "equals", ...}` with
      a real JSON number/boolean, not a quoted string, for non-text
      types).
- [ ] For a `WaitForApproval` step, switch between all three approver
      types (manager, role-in-department, specific person) and confirm
      the right sub-fields appear; the person picker is populated from
      real users.
- [ ] Hand-edit "Advanced: edit as JSON" to add a nested `and`/`or`/`not`
      branch condition, click "Load JSON into builder", and confirm that
      branch step shows a "complex condition, not editable here" notice
      but still lets you rewire its `on_true`/`on_false`; submitting
      afterward preserves that condition unchanged.
- [ ] Submit with no entry step set, a dangling reference, or a
      duplicate step id -- confirm a clear inline error and no page
      navigation (client-side validation should catch all three without
      a round trip to the server).
- [ ] Submit a workflow that fails *server-side* validation (e.g. hand-
      edit the JSON to an invalid `target_system` after building a graph)
      and confirm the re-rendered page's builder shows your graph exactly
      as you left it, not a blank canvas.
- [ ] A fresh (never-submitted) page load shows a small working two-step
      starter example, not an empty canvas.

### Approval notifications

- [ ] With `SMTP_HOST`/`SMTP_FROM_ADDRESS` unset, triggering an approval
      only logs it (`LoggingNotifier`) -- confirm the startup log warns
      that SMTP isn't configured.
- [ ] With SMTP configured against a real relay (or a local catch-all like
      Mailpit/MailHog for a dry run), triggering an approval actually
      delivers an email to the resolved approver, with a working
      `/approvals` link if `APP_BASE_URL` is set.
- [ ] An approver with a malformed email, or an unreachable/misconfigured
      relay, logs an error but does not fail the approval creation itself
      -- the approval still shows up in `/approvals`.
