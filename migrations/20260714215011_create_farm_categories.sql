-- Group-level farm membership.
--
-- A farm's category set is authoritative and INDEPENDENT of whether granular
-- products are known: much of the source data only classifies a farm at the
-- group level ("this farm sells vegetables") without a specific product
-- ("broccoli"). Deriving categories purely from farm_products would make those
-- farms vanish from category search, so we store group membership directly.
--
-- The API's `categories` is the UNION of these direct links and the groups of
-- any linked products, so a farm shows up under a category whether its data is
-- coarse (group only) or granular (specific products).
CREATE TABLE farm_categories
(
    farm_id     uuid     NOT NULL REFERENCES farms (id) ON DELETE CASCADE,
    category_id smallint NOT NULL REFERENCES product_categories (id) ON DELETE RESTRICT,
    PRIMARY KEY (farm_id, category_id)
);

-- The PK covers farm -> categories; this covers category -> farms (facet search).
CREATE INDEX farm_categories_category_id_idx ON farm_categories (category_id);
