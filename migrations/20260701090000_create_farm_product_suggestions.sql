CREATE TYPE suggestion_status AS ENUM ('PENDING', 'APPROVED', 'REJECTED');
CREATE TYPE suggestion_action AS ENUM ('ADD', 'REMOVE');

-- User-submitted proposals to add/remove a product on a farm. Reviewed by admins.
CREATE TABLE farm_product_suggestions
(
    id           uuid PRIMARY KEY,
    farm_id      uuid              NOT NULL REFERENCES farms (id) ON DELETE CASCADE,
    product_id   integer           NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    action       suggestion_action NOT NULL,
    note         text,
    submitted_by uuid              NOT NULL REFERENCES users (id),
    status       suggestion_status NOT NULL DEFAULT 'PENDING',
    reviewed_by  uuid REFERENCES users (id),
    reviewed_at  timestamptz,
    created_at   timestamptz       NOT NULL
);

-- Partial index: the moderation queue only ever reads PENDING rows, so keep the
-- index small even as approved/rejected history grows unbounded.
CREATE INDEX farm_product_suggestions_pending_idx
    ON farm_product_suggestions (created_at DESC, id DESC)
    WHERE status = 'PENDING';
