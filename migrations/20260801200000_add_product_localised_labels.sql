-- Localised display labels for the individual products.
--
-- Same shape as the category groups: `key_de` is the canonical German label and
-- the natural key, `name_en` already existed, and the remaining three languages
-- are added here. Nullable, because a product added before its translations are
-- written should still be listed rather than hidden.
--
-- Schema only, no data. The 183 product rows are inserted by
-- scripts/seed_products.py, which runs AFTER migrations — seeding labels here
-- would update an empty table on every fresh database. The seeder owns this
-- content.
ALTER TABLE products
    ADD COLUMN name_fr text,
    ADD COLUMN name_it text,
    ADD COLUMN name_rm text;

-- "Missing translation" has exactly one spelling: NULL.
--
-- `name_en` predates this rule and is included, since a blank there is what the
-- resolver's fallback chain would trip over first. `key_de` is included because
-- the chain terminates on it, and a blank canonical label leaves a caller with
-- nothing readable at all.
ALTER TABLE products
    ADD CONSTRAINT products_labels_not_blank
        CHECK (
            btrim(key_de) <> ''
                AND (name_en IS NULL OR btrim(name_en) <> '')
                AND (name_fr IS NULL OR btrim(name_fr) <> '')
                AND (name_it IS NULL OR btrim(name_it) <> '')
                AND (name_rm IS NULL OR btrim(name_rm) <> '')
            );
