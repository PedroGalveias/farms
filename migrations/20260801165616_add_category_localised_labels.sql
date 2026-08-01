-- Localised display labels for the category groups.
--
-- `key_de` already holds the canonical German label and doubles as the natural
-- key, so only the other four languages need columns. They are nullable because
-- a category added later should not be blocked on having every translation
-- ready; the API falls back rather than refusing to show anything.
--
-- Schema only, no data. The 13 category rows are inserted by
-- scripts/seed_products.py, which runs AFTER migrations — so seeding the labels
-- here would have updated an empty table on every fresh database and left the
-- columns null in CI and on any new environment. The seeder owns this content
-- and now writes the labels with it.
ALTER TABLE product_categories
    ADD COLUMN name_en text,
    ADD COLUMN name_fr text,
    ADD COLUMN name_it text,
    ADD COLUMN name_rm text;

-- "Missing translation" has exactly one spelling here: NULL.
--
-- Without this, `text` also admits '' and '   ', which read as present but
-- display as nothing: /taxonomy would answer with an empty `name` and
-- `translated: true`, and the fallback chain would stop on a value it should
-- have skipped. `key_de` is included because the chain terminates on it — a
-- blank canonical label defeats the guarantee that a caller always gets
-- something readable back.
ALTER TABLE product_categories
    ADD CONSTRAINT product_categories_labels_not_blank
        CHECK (
            btrim(key_de) <> ''
                AND (name_en IS NULL OR btrim(name_en) <> '')
                AND (name_fr IS NULL OR btrim(name_fr) <> '')
                AND (name_it IS NULL OR btrim(name_it) <> '')
                AND (name_rm IS NULL OR btrim(name_rm) <> '')
            );
