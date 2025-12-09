-- Add migration script here
CREATE TYPE user_role AS ENUM ('USER', 'ADMIN');

CREATE TABLE users
(
    id            uuid        NOT NULL,
    PRIMARY KEY (id),
    username      TEXT        NOT NULL UNIQUE,
    email         TEXT        NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,
    role          user_role   NOT NULL DEFAULT 'USER',
    created_at    timestamptz NOT NULL,
    updated_at    timestamptz
);