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
    email             TEXT        NOT NULL UNIQUE,
    email_normalised  TEXT        NOT NULL UNIQUE,
    password_hash     TEXT        NOT NULL,
    role              user_role   NOT NULL DEFAULT 'USER',
    status            user_status NOT NULL DEFAULT 'PENDING_VERIFICATION',
    email_verified_at timestamptz,
    created_at        timestamptz NOT NULL,
    updated_at        timestamptz
);

CREATE TABLE email_verification_tokens
(
    token_hash TEXT PRIMARY KEY,
    user_id    uuid        NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL,
    used_at    timestamptz,
    created_at timestamptz NOT NULL
);

CREATE INDEX email_verification_tokens_user_id_idx ON email_verification_tokens (user_id);