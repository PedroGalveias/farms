-- Localised display labels for the category groups.
--
-- `key_de` already holds the canonical German label and doubles as the natural
-- key, so only the other four languages need columns. They are nullable
-- because a category added later should not be blocked on having every
-- translation ready; the API falls back rather than showing nothing.
--
-- Seed values are the labels the web frontend already shipped, moved here so
-- the taxonomy and its display text live together. They are authored
-- translations, not generated ones — including the Romansh, which is why they
-- are worth migrating rather than regenerating.
ALTER TABLE product_categories
    ADD COLUMN name_en text,
    ADD COLUMN name_fr text,
    ADD COLUMN name_it text,
    ADD COLUMN name_rm text;

UPDATE product_categories SET name_en = 'Fruits', name_fr = 'Fruits', name_it = 'Frutta', name_rm = 'Fritgs' WHERE slug = 'fruits';
UPDATE product_categories SET name_en = 'Vegetables', name_fr = 'Légumes', name_it = 'Verdura', name_rm = 'Verduras' WHERE slug = 'vegetables';
UPDATE product_categories SET name_en = 'Dairy', name_fr = 'Produits laitiers', name_it = 'Latticini', name_rm = 'Products da latg' WHERE slug = 'dairy';
UPDATE product_categories SET name_en = 'Meat & poultry', name_fr = 'Viande et volaille', name_it = 'Carne e pollame', name_rm = 'Charn e pulam' WHERE slug = 'meat-poultry';
UPDATE product_categories SET name_en = 'Preserves & processed', name_fr = 'Produits transformés et conserves', name_it = 'Prodotti trasformati e conserve', name_rm = 'Products transformads e conservads' WHERE slug = 'preserves';
UPDATE product_categories SET name_en = 'Honey & sweeteners', name_fr = 'Miel et édulcorants', name_it = 'Miele e dolcificanti', name_rm = 'Mel e dultschadiras' WHERE slug = 'honey-sweeteners';
UPDATE product_categories SET name_en = 'Drinks', name_fr = 'Boissons', name_it = 'Bevande', name_rm = 'Bavrondas' WHERE slug = 'drinks';
UPDATE product_categories SET name_en = 'Bakery', name_fr = 'Boulangerie', name_it = 'Prodotti da forno', name_rm = 'Ar da furnaria' WHERE slug = 'bakery';
UPDATE product_categories SET name_en = 'Flowers & plants', name_fr = 'Fleurs et plantes', name_it = 'Fiori e piante', name_rm = 'Flurs e plantas' WHERE slug = 'flowers-plants';
UPDATE product_categories SET name_en = 'Nuts, seeds & oils', name_fr = 'Noix, graines et huiles', name_it = 'Noci, semi e oli', name_rm = 'Nuschs, sems ed ieli' WHERE slug = 'nuts-oils';
UPDATE product_categories SET name_en = 'Grains & cereals', name_fr = 'Céréales', name_it = 'Cereali', name_rm = 'Cereals' WHERE slug = 'grains';
UPDATE product_categories SET name_en = 'Fish & seafood', name_fr = 'Poisson et fruits de mer', name_it = 'Pesce e frutti di mare', name_rm = 'Pesch e fritgs da mar' WHERE slug = 'fish-seafood';
UPDATE product_categories SET name_en = 'Other', name_fr = 'Autres', name_it = 'Altro', name_rm = 'Auter' WHERE slug = 'other';
