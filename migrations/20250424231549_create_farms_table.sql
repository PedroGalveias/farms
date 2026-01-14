-- Create Farms Table
CREATE TABLE farms
(
    id          uuid        NOT NULL,
    PRIMARY KEY (id),
    name        TEXT        NOT NULL,
    address     TEXT        NOT NULL,
    canton      TEXT        NOT NULL,
    coordinates POINT NOT NULL,
    categories TEXT[]      NOT NULL,
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz
);

-- Add comment explaining the coordinate format
COMMENT
ON COLUMN farms.coordinates IS 'Geographic coordinates as POINT(longitude, latitude)';