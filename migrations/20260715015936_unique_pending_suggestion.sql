-- Prevent a user from stacking duplicate PENDING suggestions for the same
-- (farm, product): re-submitting the same idea is a no-op rather than queue
-- noise. Once reviewed (APPROVED/REJECTED) the row leaves the partial index,
-- so a later suggestion for the same pair is allowed again.
CREATE UNIQUE INDEX farm_product_suggestions_unique_pending_idx
    ON farm_product_suggestions (farm_id, product_id, submitted_by)
    WHERE status = 'PENDING';
