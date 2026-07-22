-- Per-(farm, product) availability state, enabled by the relationship being a
-- table rather than an array column.
CREATE TYPE stock_status AS ENUM ('AVAILABLE', 'SEASONAL', 'UNAVAILABLE');

ALTER TABLE farm_products
    ADD COLUMN status            stock_status NOT NULL DEFAULT 'AVAILABLE',
    ADD COLUMN last_confirmed_at timestamptz;

-- Partial index for "what's actually buyable now" style queries.
CREATE INDEX farm_products_available_idx
    ON farm_products (product_id)
    WHERE status = 'AVAILABLE';
