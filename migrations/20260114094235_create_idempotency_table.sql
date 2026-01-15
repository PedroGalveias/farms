CREATE TYPE header_pair AS
(
    name  TEXT,
    value BYTEA
);

CREATE TABLE idempotency
(
    user_id              uuid        NOT NULL REFERENCES users (id),
    key                  TEXT        NOT NULL,
    response_status_code SMALLINT,
    response_headers     header_pair[],
    response_body        BYTEA,
    created_at           timestamptz NOT NULL,
    expire_at            timestamptz NOT NULL,
    PRIMARY KEY (user_id, key)
);

-- Auto clean up is disabled due to requirement to install postgres cron extension

-- CREATE EXTENSION IF NOT EXISTS pg_cron;
--
-- --- Every 10 min
-- SELECT cron.schedule('delete_expired_idempotency_rows', '*/10 * * * *', $$
--   DELETE FROM idempotency
--   WHERE expire_at > now()
-- $$);
