-- category groups
CREATE TABLE product_categories
(
    id            smallint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    key_de        text     NOT NULL UNIQUE, -- canonical id, e.g., 'Früchte'
    slug          text     NOT NULL UNIQUE, -- URL/API-safe, e.g., 'fruits'
    display_order smallint NOT NULL DEFAULT 0
);

-- subcategory. Each belongs to exactly one group.
CREATE TABLE products
(
    id          integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    category_id smallint NOT NULL REFERENCES product_categories (id),
    key_de      text     NOT NULL UNIQUE,
    slug        text     NOT NULL UNIQUE,
    name_en     text
);

CREATE INDEX products_category_id_idx ON products (category_id);

-- The relationship what represents which products a farm offers/stocks.
CREATE TABLE farm_products
(
    farm_id    uuid    NOT NULL REFERENCES farms (id) ON DELETE CASCADE,
    product_id integer NOT NULL REFERENCES products (id) ON DELETE RESTRICT,
    PRIMARY KEY (farm_id, product_id)
);

-- The PK covers farm -> products; this covers the reverse (product -> farms),
-- which product search needs.
CREATE INDEX farm_products_product_id_idx ON farm_products (product_id);