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
