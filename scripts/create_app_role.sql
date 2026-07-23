-- Provision a LEAST-PRIVILEGE runtime role for the farms service.
--
-- Why: the running service should never hold more power than it uses. It does
-- NOT apply migrations (those run out-of-band as the database owner via
-- `sqlx migrate run` / scripts/render_db_migrate.py), so at runtime it only
-- needs DML — SELECT / INSERT / UPDATE / DELETE. This role therefore has:
--   * NO SUPERUSER, NO CREATEDB, NO CREATEROLE
--   * NO ownership of the database or its tables (cannot DROP/ALTER schema)
--   * NO CREATE on the schema (cannot add its own objects)
--   * only DML on the application tables + sequence access for IDENTITY columns
--
-- Run this ONCE as the database owner/admin (on Render, the credentials in the
-- service's own connection string are the owner). Pass the app password as a
-- psql variable so no secret is committed:
--
--   psql "$OWNER_DATABASE_URL" \
--     -v app_password="$(openssl rand -base64 24)" \
--     -f scripts/create_app_role.sql
--
-- Then point the service at the new role (leave migrations running as the owner):
--   APP_DATABASE__USERNAME=farms_app
--   APP_DATABASE__PASSWORD=<the password you generated above>
--
-- Re-running is safe: it is idempotent and also rotates the password to the
-- value you pass in.

\set ON_ERROR_STOP on

-- 1. The login role. Created only if missing; the password always comes from
--    the :app_password variable, never from this file.
SELECT format('CREATE ROLE farms_app LOGIN PASSWORD %L', :'app_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'farms_app')
\gexec

-- Keep the password in step on re-runs (supports rotation) and pin the role to
-- the least-privilege attributes even if it existed before.
SELECT format('ALTER ROLE farms_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE PASSWORD %L', :'app_password')
\gexec

-- 2. Connect + read the schema, but not create in it. Grant CONNECT on whatever
--    database this script is run against (the name differs on Render).
SELECT format('GRANT CONNECT ON DATABASE %I TO farms_app', current_database())
\gexec
GRANT USAGE ON SCHEMA public TO farms_app;

-- 3. DML on every existing table, plus USAGE/SELECT on sequences (harmless for
--    GENERATED … AS IDENTITY columns, required should any SERIAL be added).
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO farms_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO farms_app;

-- 4. Cover tables/sequences that FUTURE migrations create, so this never has to
--    be re-run after a deploy. ALTER DEFAULT PRIVILEGES applies to objects
--    created from now on by the role that runs it — i.e. the owner running
--    migrations. (Run this script AS that owner for the defaults to line up.)
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO farms_app;
ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT USAGE, SELECT ON SEQUENCES TO farms_app;
