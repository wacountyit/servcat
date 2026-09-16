# Contributing

## Dev environment

1. Rust (edition 2024; check `rust-version` implied by `Cargo.toml`'s
   `edition` if you hit a compiler error about it) and a MariaDB 10.6+
   instance (or run `docker compose up -d mariadb` from this repo and
   point `DATABASE_URL` at `127.0.0.1:3306`, which is published to
   localhost by default).
2. Copy `.env.example` to `.env` and fill in `DATABASE_URL` and a
   generated `JWT_SIGNING_SECRET` (`openssl rand -base64 48`).
3. `cargo run -p servcat-server` applies pending migrations on startup.
4. `cargo test --workspace` before every commit; it's fast and needs no
   database (see [TESTING.md](TESTING.md) for what it does and does not
   cover, and the manual QA checklist for everything else).
5. `cargo check --workspace` while iterating; it's faster than a full
   build and catches most mistakes.

## Adding a migration

```
./scripts/migrate.sh add <description>
```

creates a new timestamped file in `migrations/`. A few rules that matter
here specifically because migrations are embedded into the binary at
compile time (`sqlx::migrate!` in `crates/db/src/lib.rs`), not applied
from files on disk at runtime:

- Never edit or renumber an already-applied migration; add a new one.
  `sqlx migrate` tracks which migrations have run by filename/checksum, so
  editing one that's already been applied anywhere breaks that database's
  migration history.
- Prefer additive changes (`ALTER TABLE ... ADD COLUMN ... NULL`, a new
  table) over destructive ones. If a column has to be dropped or a type
  changed, say so explicitly in the PR description, since it's a one-way
  door for anyone who already has data.
- New tables follow the existing convention: `BINARY(16)` primary keys
  (`Uuid`), `TIMESTAMP ... DEFAULT CURRENT_TIMESTAMP` (and
  `ON UPDATE CURRENT_TIMESTAMP` where there's an `updated_at`), a
  `VARCHAR` + `CHECK (... IN (...))` for an enum-like column rather than a
  native `ENUM` (see the comment in `migrations/0002_users.sql` for why),
  and `utf8mb4`/`utf8mb4_unicode_ci`.

## Code conventions

This is a small, opinionated codebase; keep additions consistent with what
is already here rather than introducing a new pattern for the same kind of
problem.

- **No comments that restate what the code does.** A comment earns its
  place by explaining a non-obvious *why*: a constraint, an invariant, a
  workaround, something that would surprise a reader. Look at any existing
  file's comments for the tone to match.
- **Repositories are thin.** A function in `crates/db/src/repositories/`
  is a parameterized SQL query and not much else; business logic belongs
  in `server::orchestrator`, `approvals`, or `workflow-engine`, not buried
  in a repository function.
- **`workflow-engine` stays pure.** No database access, no HTTP, no
  `async`, inside that crate. If a change needs I/O, it belongs in
  `server::orchestrator` calling into the engine, not in the engine itself.
- **Every DB-backed `enum`-like type** goes through
  `crates/model/src/sql_enum.rs`'s `sql_string_enum!` macro (a `VARCHAR` +
  `CHECK` column, not a native SQL `ENUM`; see that file's comment for why).
- **Don't add a dependency for something a few lines of code can do**,
  especially in `crates/server` (see `sso.rs`'s hand-rolled OAuth2/PKCE
  flow and `routes/uploads.rs`'s hand-rolled static file serving as
  existing examples of this bias). If a real dependency is the right call,
  that's fine, just don't reach for one reflexively.
- **Never build SQL with string formatting on caller-supplied data.**
  Every query in this codebase is parameterized (`sqlx::query(...).bind(...)`);
  keep it that way.
- No em dashes in comments or docs in this repository; use a comma, a
  colon, or `--` instead. (This is a house style choice, not a technical
  requirement; matching it just keeps the codebase's voice consistent.)

## Before opening a pull request

- `cargo fmt --all`, `cargo check --workspace`, `cargo clippy --workspace
  --all-targets -- -D warnings`, and `cargo test --workspace` all pass.
  `ci.yml` runs all four (plus a Docker build smoke test) on every PR and
  treats them as required checks, `cargo fmt` included, so it's faster to
  catch this locally than wait on CI.
- Note in the PR description: what changed and why, which parts of the
  [manual QA checklist](TESTING.md) you actually ran (not just "should
  work"), and whether the change touches a migration, an env var, or the
  Docker/install tooling (call these out explicitly; they're the things
  most likely to break someone else's existing deployment).
- Update `.env.example`, `README.md`, `ARCHITECTURE.md`, or `SECURITY.md`
  alongside the code when a change adds/removes an env var, a route, or a
  security-relevant behavior. Docs that drift from the code are worse than
  no docs.
- Add an entry to [CHANGELOG.md](CHANGELOG.md) under "Unreleased."
