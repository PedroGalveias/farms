# Enabling pg_cron on Neon

The idempotency cleanup cron job (added by
[`migrations/20260803211457_enable_pg_cron.sql`](../migrations/20260803211457_enable_pg_cron.sql)
and
[`migrations/20260803212739_add_idempotency_cleanup_cronjob.sql`](../migrations/20260803212739_add_idempotency_cleanup_cronjob.sql))
needs a one-time, out-of-band setup step on Neon before those migrations will
succeed. This is **not** something a migration can do on its own — read on for
why, or skip to [Steps](#steps) if you just need to run it.

## Why this is needed

`pg_cron` ties both `CREATE EXTENSION` and every scheduled job's execution to
a single database, named by the `cron.database_name` setting — the background
worker only ever connects to that one database. Self-hosted (see
[`scripts/postgres/setup_pg_cron.sh`](../scripts/postgres/setup_pg_cron.sh)),
we set `cron.database_name` to the application's own database
(`POSTGRES_DB`), so `CREATE EXTENSION pg_cron` and `cron.schedule(...)` in the
migrations just work — everything happens in the same database.

On Neon, `cron.database_name` defaults to `postgres` instead of your
application's database, and you cannot change it yourself: it's a
superuser-only setting, and Neon doesn't expose `ALTER SYSTEM` to project
users. Running the migrations against your app database before this is fixed
fails with:

```
error returned from database: can only create extension in database postgres
```

You also can't work around this by enabling `pg_cron` in the `postgres`
database and pointing jobs at your app database with
`cron.schedule_in_database()` — Neon does not support that function. A job
scheduled from any database only ever executes against whatever
`cron.database_name` is currently set to, so the fix has to be to change that
setting to your app's database, not to route around it.

Until this is done, `sqlx migrate run` fails outright on this error — the
migration only catches the case where `pg_cron` isn't installed at all
(SQLSTATE `feature_not_supported`), not this "wrong database" case (SQLSTATE
`P0001` / `raise_exception`). Run the steps below **before** running
migrations against a fresh Neon database, so the `CREATE EXTENSION` in the
first migration lands in the database `cron.database_name` actually points
at instead of erroring.

Regardless of `pg_cron`, the application-level cleanup worker
([`src/idempotency/postgres_cleanup_worker.rs`](../src/idempotency/postgres_cleanup_worker.rs),
`cleanup_worker.enabled` in config) is the fallback that keeps expired
idempotency rows getting purged — keep it enabled on any environment where
you're not certain `cron.database_name` is correctly set.

## Steps

Requires a Neon API key (Neon Console → account settings) and your project's
`project_id` and `endpoint_id`.

**1. Find your `project_id` and `endpoint_id`**

- Neon Console → your project → **Settings** for the project ID (e.g.
  `young-sun-12345678`)
- Neon Console → **Branches** → **Compute** tab for the endpoint ID (e.g.
  `ep-still-rain-abcd1234`)
- Check the Neon Console's **Databases** tab for the exact database name if
  you're not sure (it's not always `neondb`)

**2. Set `cron.database_name` to your app's database**

```bash
curl --request PATCH \
     --url https://console.neon.tech/api/v2/projects/<project_id>/endpoints/<endpoint_id> \
     --header 'accept: application/json' \
     --header 'authorization: Bearer <NEON_API_KEY>' \
     --header 'content-type: application/json' \
     --data '{
  "endpoint": {
    "settings": {
      "pg_settings": {
        "cron.database_name": "<your_database_name>"
      }
    }
  }
}'
```

**3. Restart the compute endpoint to apply it**

```bash
curl --request POST \
     --url https://console.neon.tech/api/v2/projects/<project_id>/endpoints/<endpoint_id>/restart \
     --header 'accept: application/json' \
     --header 'authorization: Bearer <NEON_API_KEY>'
```

This drops current connections to the database — fine for a fresh/dev branch,
worth scheduling during a maintenance window otherwise.

**4. Run the migrations**

```bash
sqlx migrate run
```

`CREATE EXTENSION pg_cron` and `cron.schedule(...)` now run inside the
database `cron.database_name` actually points at, so they succeed instead of
being skipped — matching how the self-hosted Docker setup already behaves.

## Per-branch note

`cron.database_name` is an endpoint setting, so a **new Neon branch gets a
new compute endpoint** and inherits Neon's default (`postgres`), not whatever
you set on another branch. Repeat these steps for every new branch/project
before running migrations against it, if you want `pg_cron` to actually run
there rather than fall back to the application-level worker.
