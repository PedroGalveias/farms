# Farms

<p>
  <a href="https://github.com/PedroGalveias/farms/actions/workflows/general.yml"><img alt="CI-Build" src="https://github.com/PedroGalveias/farms/actions/workflows/general.yml/badge.svg"></a>
  <a href="https://github.com/PedroGalveias/farms/actions/workflows/github-code-scanning/codeql"><img alt="CodeQL" src="https://github.com/PedroGalveias/farms/actions/workflows/github-code-scanning/codeql/badge.svg"></a>
  <img alt="Rust edition 2024" src="https://img.shields.io/badge/rust-edition%202024-dea584">
  <img alt="License: GPL-2.0" src="https://img.shields.io/badge/license-GPL--2.0-blue">
</p>

A Rust web service for managing farm data in Switzerland, built with Actix Web, PostgreSQL, and Redis/Valkey-backed
infrastructure.

## Features

- **RESTful API** for creating and retrieving farm information
- **Directory querying** on `GET /farms`: filter by category, product, canton and free-text; geo distance sort + radius
  filter; keyset (cursor) pagination
- **Product taxonomy** — grouped categories plus granular products, snapshotted at boot; each farm carries its
  `products[]` (with per-product **stock status**) and a derived `categories[]`
- **Community product suggestions** with an admin **moderation queue**
  (submit → approve/reject)
- **Registration + email verification** lifecycle with role-aware authentication, Valkey-backed sessions, and credential
  validation against PostgreSQL
- **Rate limiting** (per-IP and per-email) backed by Valkey
- **PostgreSQL database** with SQLx for type-safe, compile-time-verified queries
- **Redis/Valkey integration** for idempotency and session storage
- **Structured logging** with tracing and bunyan formatting; optional OpenTelemetry
- **Docker support** for containerized deployment
- **Environment-based configuration** (local, production)

## Architecture

### Tech Stack

- **Web Framework**: Actix Web 4.14 with async/await
- **Database**: PostgreSQL with SQLx 0.9 (compile-time verified queries)
- **Cache / Session Infrastructure**: Redis or Valkey via deadpool-redis
- **Async Runtime**: Tokio with multi-threading
- **Logging**: tracing, tracing-subscriber, tracing-actix-web
- **Serialization**: serde, serde_json, rmp-serde

### Project Structure

```
farms/
├── src/
│   ├── main.rs                 # Application entry point
│   ├── lib.rs                  # Module exports
│   ├── startup.rs              # Server configuration, routing and HTTP setup
│   ├── configuration.rs        # Settings and database connection
│   ├── telemetry.rs            # Logging / OpenTelemetry configuration
│   ├── errors.rs               # Error utilities
│   ├── email_client.rs         # Transactional email sender (verification links)
│   ├── authentication/         # Authentication service layer
│   │   ├── mod.rs              # Authentication module exports
│   │   ├── credentials.rs      # Credential validation and authenticated user lookup
│   │   ├── password.rs         # Password hashing and verification logic
│   │   ├── registration.rs     # User registration
│   │   ├── email_verification.rs # Email-verification token issue/consume
│   │   ├── session.rs          # Valkey-backed session store
│   │   ├── extractor.rs        # Authenticated-user request extractor
│   │   └── admin.rs            # Admin-role guard
│   ├── domain/                 # Domain layer (business logic & validation)
│   │   ├── mod.rs              # Domain module exports
│   │   ├── macros.rs           # Shared macros for sqlx trait implementations
│   │   ├── test_data.rs        # Shared test data constants (reusable)
│   │   ├── suggestion.rs       # Product-suggestion domain types
│   │   ├── farm/               # Farm entity domain logic
│   │   │   ├── mod.rs          # Farm domain exports
│   │   │   ├── address.rs      # Validated address type
│   │   │   ├── canton.rs       # Validated Swiss canton type
│   │   │   ├── categories.rs   # Validated categories type
│   │   │   ├── name.rs         # Validated farm name type
│   │   │   ├── point.rs        # Validated coordinates type
│   │   │   ├── product_slug.rs # Validated product slug type
│   │   │   └── stock_status.rs # Per-product stock status enum
│   │   └── user/               # User domain logic
│   │       ├── mod.rs          # User domain exports
│   │       ├── email.rs        # Validated email type
│   │       ├── username.rs     # Validated username type
│   │       ├── password.rs     # Password newtype
│   │       ├── role.rs         # User role enum mapped to PostgreSQL
│   │       └── status.rs       # Account status enum (pending/active/…)
│   ├── taxonomy/               # Boot-time product taxonomy snapshot (slug ↔ id)
│   │   └── mod.rs
│   ├── rate_limit/             # Valkey-backed per-IP / per-email rate limiting
│   │   └── mod.rs
│   ├── routes/
│   │   ├── mod.rs
│   │   ├── health_check.rs     # Health check endpoint
│   │   ├── authentication/     # /register, /verify-email, /login, /logout, /me
│   │   │   ├── mod.rs
│   │   │   ├── error.rs
│   │   │   ├── register.rs
│   │   │   ├── verify_email.rs
│   │   │   ├── login.rs
│   │   │   ├── logout.rs
│   │   │   └── me.rs
│   │   ├── farms/              # GET /farms (directory), GET /farms/{id}, POST /farms
│   │   │   ├── mod.rs          # Farms module export + response DTOs
│   │   │   ├── error.rs        # Farms errors
│   │   │   ├── get.rs          # List (filters, geo, pagination) + detail
│   │   │   └── post.rs         # Create farm
│   │   ├── suggestions/        # POST /farms/{id}/product-suggestions
│   │   │   ├── mod.rs
│   │   │   ├── error.rs
│   │   │   └── post.rs
│   │   └── admin/              # Moderation queue (admin-only)
│   │       ├── mod.rs
│   │       ├── error.rs
│   │       └── suggestions.rs  # List / approve / reject product suggestions
│   └── idempotency/
│       ├── mod.rs              # Idempotency module export
│       ├── key.rs              # Idempotency Key struct and validation
│       ├── idempotency_data.rs # Idempotency data stored
│       ├── error.rs            # Idempotency errors
│       └── persistence/
│           ├── mod.rs          # Persistence module export
│           ├── error.rs        # Idempotency persistence errors
│           ├── redis.rs        # Idempotency persistence in Redis
│           └── postgres.rs     # Idempotency persistence in Postgres
├── migrations/                 # Database migrations
├── otel/                       # OpenTelemetry Docker Compose and config files for local testing
├── configuration/              # Environment configs (base, local, production)
├── api_docs/                   # Bruno API collection
├── scripts/                    # Database + seeding scripts
└── tests/                      # Integration tests
    ├── common/                 # Shared integration test helpers
    ├── authentication/         # Authentication service integration tests
    └── api/                    # HTTP/API integration tests
```

## Prerequisites

- Rust 1.x (edition 2024)
- PostgreSQL
- Redis or Valkey
- SQLx CLI: `cargo install sqlx-cli --no-default-features --features postgres`
- Docker (optional, for database setup)

## Getting Started

### 1. Database Setup

Initialize the PostgreSQL database using the provided script:

```bash
./scripts/init_db.sh
```

Or manually:

```bash
# Create database
sqlx database create

# Run migrations
sqlx migrate run
```

> **Deploying against a fresh Neon database?** Follow
> [`docs/neon-pg-cron-setup.md`](docs/neon-pg-cron-setup.md) **before** running
> migrations — `pg_cron` needs a one-time, out-of-band setting on Neon or the
> pg_cron-enabling migration fails.

### 2. Configuration

The application uses environment-based configuration. Set the environment:

```bash
export APP_ENVIRONMENT=local  # or production
```

Configuration files are in `configuration/`:

- `base.yaml` - Shared settings
- `local.yaml` - Local development overrides
- `production.yaml` - Production overrides

### 3. Run the Application

```bash
# Development
cargo run

# With debug logging
RUST_LOG=debug cargo run

# Production build
cargo build --release
./target/release/farms
```

The server runs on `http://localhost:8000` by default.

## Current API Surface

The service currently exposes:

- `GET /health_check`
- `GET /farms` — the directory (filters, geo, pagination — see below)
- `GET /farms/{id}`
- `GET /taxonomy` — the filtering vocabulary with display labels (see below)
- `GET /facets` — how many farms sit behind each filter option (see below)
- `POST /farms`
- `POST /farms/{id}/product-suggestions` — suggest a product for a farm
- `GET /admin/product-suggestions` — moderation queue (admin only)
- `POST /admin/product-suggestions/{id}/approve` — approve (admin only)
- `POST /admin/product-suggestions/{id}/reject` — reject (admin only)
- `POST /register`
- `POST /verify-email`
- `POST /login`
- `POST /logout`
- `GET /me`

### The Farm Directory — `GET /farms`

Every farm carries its granular `products[]` (each with `slug`, `name_de`,
`name_en`, `group` and a **stock `status`**) and a derived `categories[]`;
`coordinates` is a `"lat,lng"` string. Supported query parameters:

| Param              | Meaning                                                                                    |
|--------------------|--------------------------------------------------------------------------------------------|
| `category`         | Comma-separated group slugs (match farms in the group directly **or** via a product in it) |
| `product`          | Comma-separated product slugs                                                              |
| `match`            | `all` requires every listed product; otherwise "any of"                                    |
| `canton`           | Comma-separated canton codes, e.g. `ZH,BE`                                                 |
| `q`                | Free-text over farm name, address and product names                                        |
| `lat` / `lng`      | Requester location — adds `distance_km` to each farm                                       |
| `radius_km`        | Keep only farms within this many km of `lat`/`lng`                                         |
| `sort`             | `newest` (default) · `name` · `canton` · `nearest` (needs `lat`/`lng`)                     |
| `limit` / `offset` | Page size (clamped 1–100) and offset                                                       |
| `lang`             | Display language — see [Languages](#languages)                                             |

The response is `{ "farms": [...], "next_cursor": "<offset>" | null, "lang": "<code>" }`; a full page returns the next
offset as `next_cursor`.

### Languages

Five languages are supported: `en` (default), `de`, `fr`, `it`, `rm` — Switzerland's four national languages plus
English.

**Identity and display are separate.** Filtering always uses language-independent slugs; `lang` only decides which
human-readable label comes back. So
`?category=vegetables&lang=de` returns German text and matches exactly the same farms as `?category=vegetables&lang=fr`.
A client can store a slug forever and switch language freely.

```
GET /farms?lang=de
GET /farms/{id}?lang=fr
GET /taxonomy?lang=rm
```

**Accepted values.** Bare codes (`de`) and regional tags (`de-CH`, `de_CH`) both work — browsers and mobile clients
routinely send the regional form, and every Swiss variant maps onto the same labels. Matching is case-insensitive.

An **omitted or empty** `lang` selects the default rather than erroring. An **unsupported** one is a `400`, matching how
unknown category and product slugs are rejected: a caller who asked for something specific should hear that they did not
get it.

```http
GET /taxonomy?lang=es

HTTP/1.1 400 Bad Request
content-type: text/plain; charset=utf-8

Unsupported language 'es'. Supported languages are: en, de, fr, it, rm.
```

Every response — list, detail and taxonomy alike — echoes back the language it resolved to as a top-level `lang`, so a
caller can tell "you got German because you asked" from "you got the default" without inspecting the payload.

#### Fallback

A translation that has not been authored yet falls back, in order:

> **requested language → English → German**

English sits in the middle because it is the wider second language for this audience: a French or Italian speaker
without a translation is likelier to read
"Pineapple" as intended than "Ananas", and Romansh speakers read German anyway. German is the last resort because it is
the canonical label and is always present — so **`name` is never empty**, and a client never has to invent a placeholder
or render a blank chip.

Each entry also carries `translated`, which says whether `name` is a real translation or a fallback. That is what lets a
client decide how much to trust the label, and lets us measure coverage without guessing.

#### What is actually translated today

|                 | de | en | fr | it | rm |
|-----------------|----|----|----|----|----|
| Categories (13) | ✅ | ✅ | ✅ | ✅ | ✅ |
| Products (183)  | ✅ | ✅ | —  | —  | —  |

Product labels in fr/it/rm do not exist in the source dataset. Until they are authored
([#162](https://github.com/PedroGalveias/farms/issues/162)), asking for
`?lang=rm` returns English product names with `"translated": false` — which is the honest answer, not a bug.

### Filter Counts — `GET /facets`

How many farms sit behind each filter option, so a client can build a picker without holding the whole directory in
memory.

This exists for a specific reason. A directory that derives its own filter options from the farms it was handed can only
offer the options present in that subset — so the moment it asks the API for a *filtered* set, its canton picker
contains only the canton it already filtered to, and the visitor cannot get back out. Counts have to come from somewhere
that always sees everything.

```text
GET /facets
GET /facets?lang=de
```

```json
{
  "lang": "de",
  "total": 3155,
  "cantons": [
    {
      "code": "AG",
      "count": 118
    },
    {
      "code": "BE",
      "count": 727
    }
  ],
  "categories": [
    {
      "slug": "fruits",
      "name": "Früchte",
      "translated": true,
      "count": 412
    },
    {
      "slug": "fish-seafood",
      "name": "Fisch und Meeresfrüchte",
      "translated": true,
      "count": 0
    }
  ]
}
```

- **`cantons`** lists only cantons that have farms — a canton nobody farms in would be a dead option in a picker.
- **`categories`** is exhaustive, including counts of zero, mirroring
  `GET /taxonomy`: a picker needs the full vocabulary, and the count is what tells it which options to grey out.
- A farm counts once per category whether it is tagged **directly** or through a **product** in that category — the same
  "any of" rule `?category=` applies on `GET /farms`, so a count always agrees with what filtering would return.

Counts are **unfiltered on purpose**: they describe the whole directory, so an option's count does not shift as other
filters are applied. That is what makes them a stable ordering for a picker. Contextual counts are a different question
and would take the same filter parameters as `GET /farms`.

Cached `public, max-age=300, stale-while-revalidate=3600` — matched to how long a client caches `GET /farms`, so a count
never disagrees with the list beside it.

### The Vocabulary — `GET /taxonomy`

The whole filtering vocabulary in one request, so a client does not carry its own copy of the taxonomy and keep it in
sync by hand.

```jsonc
{
  "lang": "fr",
  "categories": [
    { "slug": "vegetables", "name": "Légumes", "translated": true }
  ],
  "products": [
    { "slug": "apples", "name": "Apples", "translated": false, "category": "fruits" }
  ]
}
```

Products arrive ordered by category display order then slug, and each names its
`category`, so a grouped picker can be rendered straight from this response.

Labels live here rather than on `/farms` for two reasons: a farm response only carries the products *that farm* stocks,
never the full list a picker needs; and repeating 180 product names across 3,155 farms is a lot of bytes for something a
client looks up once.

The vocabulary is a snapshot taken at startup, so it cannot change until the app restarts. The endpoint says so:

```
Cache-Control: public, max-age=3600, stale-while-revalidate=86400
```

An hour of caching risks nothing a restart would not already impose, and
`stale-while-revalidate` lets a picker render instantly from a day-old copy while it refreshes behind the scenes.
`public` is safe — the response varies only by `?lang=`, which is in the URL. A `400` for an unsupported language
carries no cache header, so fixing the request takes effect immediately.

> `products[]` on `/farms` still carries `name_de` and `name_en`. Those are a
> compatibility shim for clients written before `/taxonomy` existed — prefer
> looking names up by slug from here.

### Product Suggestions & Moderation

`POST /farms/{id}/product-suggestions` accepts `{ "product": "<slug>", "note"?: string }`
and queues a `PENDING` suggestion. Admins review the queue via
`GET /admin/product-suggestions` and `approve`/`reject` each; approving links the product to the farm (as `AVAILABLE`).
All `/admin/*` routes require an admin role.

### Authentication & Registration Lifecycle

Registration is public and email-verified:

1. `POST /register` with `{ "username", "email", "password" }` creates a `USER`
   account in a `PENDING_VERIFICATION` state and emails a verification link. It responds `202 Accepted` - the same
   response for new and already-registered emails - so it cannot be used to enumerate accounts. A taken **username**,
   however, returns `409 Conflict`: usernames are public identifiers, so a clash is reported rather than hidden.
   Usernames are 3-30 characters (letters/digits/`_`/`-`, stored lowercased); passwords must be at least 12 characters;
   `role` is server-owned and cannot be set by the client.
2. `POST /verify-email` with `{ "token" }` consumes the (single-use, expiring)
   token, marks the account `ACTIVE`, and sets `email_verified_at`.
3. `POST /login` validates credentials and, on success, persists a Valkey-backed session via a signed cookie. **Only
   `ACTIVE` users can log in**
    - pending and disabled accounts get the same generic `401` as a wrong password.
4. `GET /me` returns the current user; `POST /logout` purges the session.

Verification tokens are stored only as SHA-256 hashes; the raw token exists solely in the email sent to the user.
Registration is rate limited per IP and per email using Valkey.

### Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run with logging
TEST_LOG=1 cargo test
```

### Code Quality

```bash
# Lint all targets
cargo clippy --all-targets
```

### Database Management

```bash
# Create new migration
sqlx migrate add <migration_name>

# Run migrations
sqlx migrate run

# Revert last migration
sqlx migrate revert

# Prepare queries
cargo sqlx prepare --workspace --all -- --all-targets
 
# Reset database
SKIP_DOCKER=true ./scripts/init_db.sh
```

### Production database role (least privilege)

The service **does not run migrations** — those are applied out-of-band by the database owner (`sqlx migrate run` /
[`scripts/render_db_migrate.py`](scripts/render_db_migrate.py)). At runtime it therefore needs only DML, so it should
**not** connect as a superuser or as the database owner. On managed Postgres (e.g. Render) the credentials you are given
are never a cluster **superuser**, but they *are* the database **owner** — more than the running service needs.

[`scripts/create_app_role.sql`](scripts/create_app_role.sql) provisions a dedicated `farms_app` login role with **no
superuser / createdb / createrole**, **no ownership**, and **no CREATE on the schema** — just
`SELECT/INSERT/UPDATE/DELETE` on the app tables (plus `ALTER DEFAULT PRIVILEGES`
so future migrations' tables are covered automatically). Run it once as the owner, passing the password as a psql
variable so nothing secret is committed:

```bash
# Generate the password into a variable so you can RETAIN it for the runtime
# config — the script only receives it as a psql variable, never from a file.
APP_PASSWORD="$(openssl rand -base64 24)"
psql "$OWNER_DATABASE_URL" -v "app_password=$APP_PASSWORD" \
  -f scripts/create_app_role.sql
```

The script runs in a single transaction (atomic), so a mid-run failure can't leave `farms_app` with a rotated password
but missing grants. Then point the service at the restricted role (keep migrations running as the owner) by storing that
same value in the secret manager / environment:

```bash
APP_DATABASE__USERNAME=farms_app
APP_DATABASE__PASSWORD=$APP_PASSWORD
```

Verify what a connection is actually running as:

```sql
-- expect: rolsuper = f (and, ideally, not the database owner)
SELECT current_user, rolsuper
FROM pg_roles
WHERE rolname = current_user;
```

### Idempotency Expiry Cleanup

Expired rows in the `idempotency` table are purged two ways, so neither one is a single point of failure:

1. **`pg_cron`** — [`migrations/20260803211457_enable_pg_cron.sql`](migrations/20260803211457_enable_pg_cron.sql)
   enables the extension and
   [
   `migrations/20260803212739_add_idempotency_cleanup_cronjob.sql`](migrations/20260803212739_add_idempotency_cleanup_cronjob.sql)
   schedules a nightly `DELETE FROM idempotency WHERE expire_at < NOW()`. Both migrations wrap the setup in a
   `DO $$ ... EXCEPTION WHEN ... END; $$` block that catches the case where `pg_cron` isn't installed on the target
   Postgres instance and just logs a warning instead of failing the migration run — so the same migrations apply cleanly
   against a managed Postgres that doesn't offer `pg_cron` (e.g. plain `postgres:alpine`).
2. **Application-level cleanup worker** — [
   `src/idempotency/postgres_cleanup_worker.rs`](src/idempotency/postgres_cleanup_worker.rs)
   runs the same delete on a timer inside the service itself. Controlled by the
   `cleanup_worker` config block:

   ```yaml
   cleanup_worker:
     enabled: false
     run_interval: 60 # minutes; must be > 0
   ```

   `run_interval` is validated at config-load time and rejects `0` (which would otherwise busy-loop the worker with a
   zero-length sleep).

Because `pg_cron` only activates for a **freshly initialized** Postgres data directory
(see [Docker Compose](#docker-compose-local-stack) below), the cleanup worker should stay **enabled** for any deployment
where you can't guarantee `pg_cron` is active — it's a harmless no-op alongside a working cron job, and the only thing
doing cleanup when `pg_cron` isn't.

**Deploying to Neon:** `pg_cron` needs a one-time, out-of-band setup step there before the migrations will succeed — see
[`docs/neon-pg-cron-setup.md`](docs/neon-pg-cron-setup.md).

## Docker Deployment

Build and run using Docker:

```bash
# Build image
docker build -t farms:latest .

# Run container
# The service is configured via APP_ENVIRONMENT + APP_* variables (config-rs,
# `__` nests keys) — NOT a runtime DATABASE_URL (that's only used at build time
# by sqlx-cli). Point it at your database and Redis/Valkey:
docker run -p 8000:8000 \
  -e APP_ENVIRONMENT=production \
  -e APP_DATABASE__HOST=db-host \
  -e APP_DATABASE__PORT=5432 \
  -e APP_DATABASE__USERNAME=app \
  -e APP_DATABASE__PASSWORD=secret \
  -e APP_DATABASE__DATABASE_NAME=farms \
  -e APP_DATABASE__REQUIRE_SSL=true \
  farms:latest
```

The Dockerfile uses a multi-stage build with cargo-chef for efficient layer caching.

### Docker Compose (Local Stack)

`docker-compose.yml` runs the full stack locally: the `farms` API, a `db`
service built from [`scripts/postgres/Dockerfile`](scripts/postgres/Dockerfile)
(the official Postgres image plus `pg_cron`, configured by
[`scripts/postgres/setup_pg_cron.sh`](scripts/postgres/setup_pg_cron.sh)), and
`redis` (Valkey).

```bash
docker-compose up -d
```

`setup_pg_cron.sh` is a `docker-entrypoint-initdb.d` script, so it only runs against a **fresh, empty** `postgres-data`
volume — an existing volume from before `pg_cron` support was added won't pick it up. That's why
`APP_CLEANUP_WORKER__ENABLED` stays `true` in `docker-compose.yml`: it's the fallback that keeps cleanup working
regardless of whether `pg_cron` actually activated for a given volume
(see [Idempotency Expiry Cleanup](#idempotency-expiry-cleanup)).

Since `sqlx-cli` isn't installed in any of the containers, migrations are run from the host against the `db` container's
published port (`5432`):

```bash
./scripts/migrate_compose.sh
```

This waits for `db`'s healthcheck to pass, then runs
`sqlx database create && sqlx migrate run` with `DATABASE_URL` pointed at
`localhost:5432` — no CLI needed inside any container, and no need for the service to run migrations on startup.

## OpenTelemetry Support

OpenTelemetry support is built in. It's off by default; enable it via the configuration files to have the service export
to an OTLP collector. Configuration example below with explanations:

```yaml
telemetry:
  enabled: true
  service_name: "farms-service"
  endpoint: "${OTEL_EXPORTER_OTLP_ENDPOINT}"  # Set via environment variable  and should be the OTLP gRPC or HTTP endpoint
  environment: "production"
```

## API Documentation

API requests are documented using [Bruno](https://www.usebruno.com/) in the `api_docs/` directory. Import the collection
into Bruno to explore and test the API endpoints.

## Environment Variables

- `APP_ENVIRONMENT` - Environment name (local/production)
- `DATABASE_URL` - PostgreSQL connection string (for SQLx CLI)
- `RUST_LOG` - Logging level (trace/debug/info/warn/error)
- `TEST_LOG` - Enable logging of API during test execution

## License

This project is licensed under the GPL-2.0 License. See the LICENSE file for details.
