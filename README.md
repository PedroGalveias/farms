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
- **Directory querying** on `GET /farms`: filter by category, product, canton and
  free-text; geo distance sort + radius filter; keyset (cursor) pagination
- **Product taxonomy** — grouped categories plus granular products, snapshotted at
  boot; each farm carries its `products[]` (with per-product **stock status**) and
  a derived `categories[]`
- **Community product suggestions** with an admin **moderation queue**
  (submit → approve/reject)
- **Registration + email verification** lifecycle with role-aware authentication,
  Valkey-backed sessions, and credential validation against PostgreSQL
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

| Param | Meaning |
| --- | --- |
| `category` | Comma-separated group slugs (match farms in the group directly **or** via a product in it) |
| `product` | Comma-separated product slugs |
| `match` | `all` requires every listed product; otherwise "any of" |
| `canton` | Comma-separated canton codes, e.g. `ZH,BE` |
| `q` | Free-text over farm name, address and product names |
| `lat` / `lng` | Requester location — adds `distance_km` to each farm |
| `radius_km` | Keep only farms within this many km of `lat`/`lng` |
| `sort` | `newest` (default) · `name` · `canton` · `nearest` (needs `lat`/`lng`) |
| `limit` / `offset` | Page size (clamped 1–100) and offset |

The response is `{ "farms": [...], "next_cursor": "<offset>" | null }`; a full
page returns the next offset as `next_cursor`.

### Product Suggestions & Moderation

`POST /farms/{id}/product-suggestions` accepts `{ "product": "<slug>", "note"?: string }`
and queues a `PENDING` suggestion. Admins review the queue via
`GET /admin/product-suggestions` and `approve`/`reject` each; approving links the
product to the farm (as `AVAILABLE`). All `/admin/*` routes require an admin role.

### Authentication & Registration Lifecycle

Registration is public and email-verified:

1. `POST /register` with `{ "username", "email", "password" }` creates a `USER`
   account in a `PENDING_VERIFICATION` state and emails a verification link. It
   responds `202 Accepted` - the same response for new and already-registered
   emails - so it cannot be used to enumerate accounts. A taken **username**,
   however, returns `409 Conflict`: usernames are public identifiers, so a clash
   is reported rather than hidden. Usernames are 3-30 characters
   (letters/digits/`_`/`-`, stored lowercased); passwords must be at least 12
   characters; `role` is server-owned and cannot be set by the client.
2. `POST /verify-email` with `{ "token" }` consumes the (single-use, expiring)
   token, marks the account `ACTIVE`, and sets `email_verified_at`.
3. `POST /login` validates credentials and, on success, persists a
   Valkey-backed session via a signed cookie. **Only `ACTIVE` users can log in**
    - pending and disabled accounts get the same generic `401` as a wrong
      password.
4. `GET /me` returns the current user; `POST /logout` purges the session.

Verification tokens are stored only as SHA-256 hashes; the raw token exists
solely in the email sent to the user. Registration is rate limited per IP and
per email using Valkey.

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

## OpenTelemetry Support

To enable OpenTelemetry support the service must be compiled with the `opentelemetry` feature enabled otherwise it will not work. To do this use:

```sh
cargo build --release --features opentelemetry
```

After the compilation is complete using the configuration files need to be updated to enable the service to utilize OpenTelemetry collectors. Configuration example bellow with explanations:

```yaml
telemetry:
  enabled: true
  service_name: "farms-service"
  endpoint: "${OTEL_EXPORTER_OTLP_ENDPOINT}"  # Set via environment variable  and should be the OTLP gRPC endpoint
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
