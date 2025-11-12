-- Create Farms Table
CREATE TABLE farms
(
    id          uuid        NOT NULL,
    PRIMARY KEY (id),
    name        TEXT        NOT NULL,
    address     TEXT        NOT NULL,
    canton      TEXT        NOT NULL,
    coordinates TEXT        NOT NULL,
    categories TEXT[]      NOT NULL,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz
);