# Farms

A Rust web service for managing farm data in Switzerland, built with Actix Web, PostgreSQL, and Redis/Valkey-backed
infrastructure.

## Features

- **RESTful API** for creating and retrieving farm information
- **Role-aware authentication** with credential validation against PostgreSQL
- **Login endpoint** returning authenticated user identity and role
- **PostgreSQL database** with SQLx for type-safe queries
- **Redis/Valkey integration** for idempotency support and future session storage
- **Structured logging** with tracing and bunyan formatting
- **Docker support** for containerized deployment
- **Environment-based configuration** (local, production)

## Architecture

### Tech Stack

- **Web Framework**: Actix Web 4.12 with async/await
- **Database**: PostgreSQL with SQLx 0.8 (compile-time verified queries)
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
│   ├── startup.rs              # Server configuration and HTTP setup
│   ├── configuration.rs        # Settings and database connection
│   ├── telemetry.rs            # Logging configuration
│   ├── errors.rs               # Error utilities
│   ├── authentication/         # Authentication service layer
│   │   ├── mod.rs              # Authentication module exports
│   │   ├── password.rs         # Password hashing and verification logic
│   │   └── credentials.rs      # Credential validation and authenticated user lookup
│   ├── domain/                 # Domain layer (business logic & validation)
│   │   ├── mod.rs              # Domain module exports (farm, user, macros, test_data)
│   │   ├── macros.rs           # Shared macros for sqlx trait implementations
│   │   ├── test_data.rs        # Shared test data constants (reusable)
│   │   ├── farm/               # Farm entity domain logic
│   │   │   ├── mod.rs          # Farm domain exports (Address, Canton, etc.)
│   │   │   ├── address.rs      # Validated address type
│   │   │   ├── canton.rs       # Validated Swiss canton type
│   │   │   ├── categories.rs   # Validated categories type
│   │   │   ├── name.rs         # Validated farm name type
│   │   │   └── point.rs        # Validated coordinates type
│   │   └── user/               # User domain logic
│   │       ├── mod.rs          # User domain exports
│   │       └── role.rs         # User role enum mapped to PostgreSQL
│   ├── routes/
│   │   ├── authentication/
│   │   │   ├── mod.rs          # Authentication route exports
│   │   │   ├── error.rs        # Login route errors
│   │   │   └── login.rs        # POST /login endpoint
│   │   ├── health_check.rs     # Health check endpoint
│   │   └── farms/
│   │       ├── mod.rs          # Farms module export and Farm struct
│   │       ├── error.rs        # Farms errors
│   │       ├── get.rs          # Farm get operations
│   │       └── post.rs         # Farm post operations
│   └── idempotency/
│       ├── mod.rs              # Idempotency module export
│       ├── key.rs              # Idempotency Key struct and validation
│       ├── idempotency_data.rs # Idempotency data stored
│       ├── error.rs            # Idempotency errors
│       └── persistence/
│           ├── mod.rs          # Persistence of idempotency details module export
│           ├── error.rs        # Idempotency persistence errors
│           ├── redis.rs        # Idempotency persistence in Redis
│           └── postgres.rs     # Idempotency persistence in Postgres (Untested)
├── migrations/                 # Database migrations
├── otel/                       # OpenTelemetry Docker Compose and config files for local testing
├── configuration/              # Environment configs (base, local, production)
├── api_docs/                   # Bruno API collection
├── scripts/                    # Database setup scripts
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
- `GET /farms`
- `POST /farms`
- `POST /login`

### Authentication Status

`POST /login` currently validates credentials and returns the authenticated user's id and role.

This endpoint does **not** yet create or persist a session, and authenticated route protection is not yet enforced.
Those concerns are planned for a follow-up iteration.

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

### Import Farm Dataset

To import the JSON dataset used in this project into the `farms` table:

```bash
python3 scripts/import_farms_from_json.py --json-path /absolute/path/to/farms_with_categorized_products.json --dry-run
python3 scripts/import_farms_from_json.py --json-path /absolute/path/to/farms_with_categorized_products.json
```

The importer:

- normalizes farm names, addresses, cantons, coordinates, and categories into the current schema
- skips rows missing required farm data after normalization
- skips duplicate records already present in the database

## Docker Deployment

Build and run using Docker:

```bash
# Build image
docker build -t farms:latest .

# Run container
docker run -p 8000:8000 \
  -e DATABASE_URL=postgres://user:pass@host:5432/farms \
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
