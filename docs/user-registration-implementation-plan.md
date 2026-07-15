# User Registration Implementation Plan

This plan is intentionally documentation-only. It describes how user registration
should be implemented in this Actix Web service without adding the registration
logic in this change.

The recommended design follows the style already used in this repository and in
the Zero to Production approach:

- Keep HTTP handlers thin.
- Validate untrusted input with domain types before it reaches persistence.
- Push business workflows into service functions.
- Use SQLx queries explicitly.
- Treat passwords, tokens, and API keys as secrets.
- Keep expensive password work off Actix worker threads.
- Return stable, non-leaky HTTP errors.
- Prove behaviour with integration tests before relying on it.

## Public Contract

Implement public registration as:

```text
POST /register
```

Request body:

```json
{
  "email": "person@example.com",
  "password": "a long user-chosen password"
}
```

Response:

```text
202 Accepted
```

The response should be generic. It must not reveal whether the email address is
new, already pending verification, or already registered.

Registration should create a `USER` account in a pending verification state,
store an email verification token hash, and send a verification email. The user
must not be treated as authenticated until email verification is complete.

Do not accept `role` from the client. Roles are server-owned state.

## Migration Strategy For This Dev Project

This project is still in development, so do not add corrective migrations such
as `add_user_registration_state`.

Instead, update the existing schema migrations so a fresh database is created in
the desired final shape. After changing the existing migrations, reset local and
test databases and rerun migrations from scratch.

That means:

- Update `migrations/20251209223944_create_users_table.sql`.
- Keep the user/account schema together there.
- Do not create a table named `add_user_registration_state`.
- Do not add a new migration just to patch the current `users` table.
- If a local developer database already has data, drop/reset it and migrate
  again.

This would not be acceptable for a production database with existing user data,
but it is the cleaner path while the project is still pre-production.

## Step-by-Step Implementation

### 1. Update The Existing Users Migration

File:

- `migrations/20251209223944_create_users_table.sql`

Update the existing migration so it creates:

- `user_status` enum with `PENDING_VERIFICATION`, `ACTIVE`, and `DISABLED`.
- `users.status user_status NOT NULL DEFAULT 'PENDING_VERIFICATION'`.
- `users.email_verified_at timestamptz`.
- `users.email_normalized TEXT NOT NULL`.
- A unique index on `email_normalized`.

The `users` table should be created directly with the final columns. A clean
version should look like this shape:

```sql
CREATE TYPE user_role AS ENUM ('USER', 'ADMIN');

CREATE TYPE user_status AS ENUM (
    'PENDING_VERIFICATION',
    'ACTIVE',
    'DISABLED'
);

CREATE TABLE users
(
    id                uuid        NOT NULL,
    PRIMARY KEY (id),
    username          TEXT        NOT NULL UNIQUE,
    email             TEXT        NOT NULL,
    email_normalized  TEXT        NOT NULL UNIQUE,
    password_hash     TEXT        NOT NULL,
    role              user_role   NOT NULL DEFAULT 'USER',
    status            user_status NOT NULL DEFAULT 'PENDING_VERIFICATION',
    email_verified_at timestamptz,
    created_at        timestamptz NOT NULL,
    updated_at        timestamptz
);
```

Why:

- Login must be able to reject unverified and disabled accounts.
- Registration needs a safe pending state.
- Case-insensitive uniqueness should be explicit instead of relying on ad hoc
  lowercase comparisons.

Existing test helpers that insert users directly should explicitly set
`status = 'ACTIVE'`, `email_verified_at = created_at`, and
`email_normalized = lower(trim(email))` for already-verified test users.

### 2. Add Verification Tokens To The Existing Auth Migration

File:

- `migrations/20251209223944_create_users_table.sql`

Add the verification token table to the same existing migration, after the
`users` table:

```sql
CREATE TABLE email_verification_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id uuid NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    used_at timestamptz,
    created_at timestamptz NOT NULL
);

CREATE INDEX email_verification_tokens_user_id_idx
    ON email_verification_tokens (user_id);
```

Why:

- Store only a hash of the token, never the raw token.
- Allow token expiry, single-use semantics, and later audit/debugging.
- Keep verification independent from the user row while still attached by
  foreign key.

Use a cryptographically random token. Store `SHA-256(token)` or a comparable
one-way hash and send only the raw token to the user by email.

### 3. Add User Domain Types

Files:

- `src/domain/user/email.rs`
- `src/domain/user/password.rs`
- `src/domain/user/status.rs`
- `src/domain/user/mod.rs`

Implement:

- `EmailAddress` or `Email` newtype.
- `Password` or `UserPassword` newtype wrapping `SecretString`.
- `UserStatus` enum mapped with `sqlx::Type`.
- Re-export the new types from `src/domain/user/mod.rs`.

Why:

- Input validation belongs at the boundary of the domain, not scattered through
  routes and SQL calls.
- Email normalization becomes one canonical operation.
- Password limits protect the service from silly or hostile payloads.

Email rules:

- Trim surrounding whitespace.
- Normalize the value used for uniqueness.
- Use a maintained email parsing crate instead of a handwritten regex.

Password rules:

- Require a minimum length, for example 12 characters.
- Enforce a maximum byte length, for example 1024 bytes, to limit hashing cost.
- Allow Unicode.
- Avoid composition rules like "one symbol, one uppercase"; they are weaker and
  often hostile to password managers.

### 4. Add Registration Errors

Files:

- `src/authentication/registration.rs`
- `src/routes/authentication/error.rs`

Add service-level errors such as:

- `InvalidRegistrationInput`
- `UnexpectedError`
- `EmailDeliveryError`
- `RateLimited`

Map route-level errors with `ResponseError`.

Why:

- The service layer needs rich internal errors.
- The HTTP layer must expose only stable, intentionally boring responses.
- Duplicate emails should not leak through a `409 Conflict` response on the
  public registration endpoint.

Recommended HTTP mapping:

- Invalid JSON/body shape: `400 Bad Request`.
- Invalid email/password: `400 Bad Request`.
- Accepted registration request: `202 Accepted`.
- Existing email: `202 Accepted`.
- Rate limit exceeded: `429 Too Many Requests`.
- Unexpected infrastructure failure: `500 Internal Server Error`.

### 5. Implement Registration Service Logic

File:

- `src/authentication/registration.rs`

Implement a function shaped like:

```rust
pub async fn register_user(
    email: Email,
    password: UserPassword,
    pool: &PgPool,
    email_client: &EmailClient,
    settings: &RegistrationSettings,
) -> Result<(), RegisterUserError>
```

Workflow:

1. Normalize and validate input before opening a transaction.
2. Start a PostgreSQL transaction.
3. Check whether `email_normalized` already exists.
4. If it exists, commit/rollback without revealing that fact and return success.
5. Hash the password using the existing Argon2 code via
   `spawn_blocking_with_tracing`.
6. Insert a new user with:
    - generated UUID,
    - generated internal username such as `user-{uuid}`,
    - original email,
    - normalized email,
    - password hash,
    - `USER` role,
    - `PENDING_VERIFICATION` status,
    - timestamps.
7. Generate a verification token.
8. Store only the verification token hash with expiry.
9. Commit the transaction.
10. Send the verification email.

Why:

- Transaction boundaries keep user and token creation consistent.
- Password hashing remains off the Actix worker thread.
- Generic success for existing email limits account enumeration.
- Server-generated username avoids adding a public username availability problem
  to the first registration release.

If email delivery must be fully production-grade from day one, add a
transactional outbox table and a worker instead of sending directly after
commit. Direct sending after commit is simpler, but failures leave users pending
until they request a resend.

### 6. Add Email Verification Logic

Files:

- `src/authentication/email_verification.rs`
- `src/routes/authentication/verify_email.rs`
- `src/routes/authentication/mod.rs`
- `src/startup.rs`

Add:

```text
POST /verify-email
```

Request body:

```json
{
  "token": "raw-token-from-email"
}
```

Workflow:

1. Hash the submitted token.
2. Look up an unused, unexpired token by hash.
3. In one transaction:
    - mark token as used,
    - set `users.status = 'ACTIVE'`,
    - set `users.email_verified_at = now()`.
4. Return `200 OK` for a valid token.
5. Return a generic `400 Bad Request` for invalid, expired, or used tokens.

Why:

- Tokens are bearer credentials and must be single-use.
- User activation should be atomic with token consumption.
- Invalid token responses should not reveal which part failed.

### 7. Update Login To Respect Account Status

File:

- `src/authentication/credentials.rs`

Change `get_credentials` to also fetch `status`.

Change `validate_credentials` so only `ACTIVE` users can authenticate. For
pending, disabled, unknown email, or wrong password, return the same
`InvalidCredentials` variant.

Why:

- Registration is not secure if pending users can log in immediately.
- Disabled accounts must not authenticate.
- The login endpoint should keep its current non-enumerating behaviour.

### 8. Add Email Client Infrastructure

Files:

- `src/email_client.rs`
- `src/configuration.rs`
- `configuration/base.yaml`
- `configuration/local.yaml`
- `configuration/production.yaml`
- `src/startup.rs`

Implement an email client similar to the Zero to Production pattern:

- base URL,
- sender email,
- authorization token in `SecretString`,
- timeout,
- `reqwest` client,
- structured errors.

Why:

- Registration should not know provider-specific HTTP details.
- Tests can use a mock HTTP server.
- Secrets stay out of logs.
- Timeouts prevent requests from hanging worker tasks.

Production settings should be environment-driven. Do not hard-code provider
credentials in YAML.

### 9. Add Registration Settings

Files:

- `src/configuration.rs`
- `configuration/base.yaml`
- `configuration/production.yaml`

Add:

- verification token TTL, for example 24 hours,
- public frontend URL used to build verification links,
- per-email registration cooldown,
- rate limit settings.

Why:

- Security-sensitive values should be configurable.
- URLs differ between local, staging, and production.
- Rate limits should be tunable without code changes.

### 10. Add Rate Limiting

Files:

- `src/rate_limit/mod.rs`
- `src/rate_limit/redis.rs`
- `src/lib.rs`
- `src/routes/authentication/register.rs`

Use Redis-backed fixed-window or sliding-window limits for:

- source IP,
- normalized email,
- verification resend if added.

Why:

- Registration endpoints are abuse magnets.
- Per-process in-memory limits are weak once the app has more than one instance.
- Redis already exists in this service and is appropriate for short-lived
  counters.

Return `429 Too Many Requests` with no sensitive detail.

### 11. Add HTTP Route

Files:

- `src/routes/authentication/register.rs`
- `src/routes/authentication/mod.rs`
- `src/startup.rs`

The handler should:

1. Accept `web::Json<RegisterRequest>`.
2. Parse `Email` and `UserPassword`.
3. Call the registration service.
4. Return `202 Accepted` with an empty body.

Why:

- The Actix handler stays thin and testable.
- The route does not contain SQL, hashing, token creation, or email provider
  details.

Also add or tighten JSON payload limits in `src/startup.rs` via `JsonConfig`.
Registration bodies are small; large bodies should be rejected early.

### 12. Add API Documentation

Files:

- `api_docs/Register.bru`
- `api_docs/Verify Email.bru`
- `README.md`

Document:

- `POST /register`,
- `POST /verify-email`,
- expected request bodies,
- generic registration response,
- the fact that users cannot log in until verified.

Why:

- Bruno docs are already the local API documentation format.
- README should describe the public authentication lifecycle.

### 13. Add Tests First

Files:

- `tests/api/registration.rs`
- `tests/api/main.rs`
- `tests/authentication/registration.rs`
- `tests/authentication/main.rs`
- `tests/common/mod.rs`

HTTP tests:

- `register_returns_202_for_valid_input`.
- `register_returns_202_for_existing_email`.
- `register_returns_400_for_invalid_email`.
- `register_returns_400_for_short_password`.
- `register_stores_pending_user_with_hashed_password`.
- `register_sends_verification_email_without_leaking_token_in_db`.
- `login_rejects_pending_user`.
- `verify_email_activates_user`.
- `verify_email_rejects_expired_token`.
- `verify_email_rejects_used_token`.
- `active_verified_user_can_login`.
- `register_is_rate_limited`.

Service tests:

- Email normalization is stable.
- Password policy accepts long generated passwords and Unicode.
- Token hashes are not equal to raw tokens.
- Existing email follows the same public success path.

Why:

- This project already leans on integration tests for real HTTP and database
  behaviour.
- Registration has security and data consistency risks; tests should cover the
  boundary, not only pure helpers.

### 14. Update Test Helpers

File:

- `tests/common/mod.rs`

Update `TestUser::store` for new columns:

- `email_normalized`,
- `status`,
- `email_verified_at`.

Add helpers for:

- posting registration JSON,
- posting verification token JSON,
- retrieving inserted users by email,
- retrieving verification token rows,
- mocking email delivery.

Why:

- Tests should remain readable.
- Existing authentication and farm tests should keep working after the users
  table changes.

### 15. Update SQLx Metadata

Files:

- SQLx query cache, if this repository commits `.sqlx/`.
- Otherwise no tracked file, but run the command locally.

Run:

```sh
cargo sqlx prepare --workspace --all -- --all-targets
```

Why:

- SQLx compile-time query checking must know about the new schema and enums.

### 16. Verification Commands

Run:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo sqlx prepare --workspace --all -- --all-targets
```

Why:

- Format, behaviour, linting, and SQLx query metadata all need to agree before
  this is production-ready.

## Security Decisions

- Return generic success for duplicate email registration attempts.
- Store password hashes only, using Argon2id.
- Keep Argon2 work on blocking threads.
- Store verification token hashes, not raw tokens.
- Expire verification tokens.
- Consume verification tokens once.
- Do not log request bodies, passwords, or tokens.
- Do not accept roles from registration input.
- Do not create an authenticated session until verification succeeds.
- Rate limit by IP and normalized email.
- Use `SecretString` for passwords, tokens, and provider API keys.
- Use transactions for user/token state changes.
- Use production-only secure cookies and environment-provided secrets, as the
  current session configuration already does.

## Suggested Implementation Order

1. Update the existing users migration for status, email normalization, and
   verification tokens.
2. Domain types for email, password, and user status.
3. Login status check so pending users cannot authenticate.
4. Registration service without email sending, covered by database tests.
5. Email verification token generation and storage.
6. Email client and configuration.
7. `POST /register` route.
8. `POST /verify-email` route.
9. Rate limiting.
10. Bruno and README documentation.
11. SQLx prepare, test, fmt, and clippy.
