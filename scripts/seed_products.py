#!/usr/bin/env python3
"""Generate idempotent seed SQL for the product taxonomy + farms.

The application logic (schema, snapshot, read/write API, moderation) is built
separately; this script is the *data* half — it turns the source dataset into
SQL that populates:

  - product_categories   (13 groups: German key + English + stable slug)
  - products             (granular products: German key + English + slug)
  - farms                (name/address/canton/coordinates)
  - farm_categories      (group-level membership — the coarse classification)
  - farm_products        (granular product links)

Roughly a quarter of farms only carry a group-level classification (no specific
product); those get `farm_categories` rows only, which is exactly why the API
derives a farm's categories from BOTH tables.

The SQL is slug-addressed (never hard-codes generated ids) and fully
idempotent (ON CONFLICT), so it is safe to re-run and to apply to any
environment.

    Usage:
        python3 scripts/seed_products.py > seed.sql
        psql "$DATABASE_URL" -f seed.sql

    Then RESTART the app so its boot-time taxonomy snapshot picks up the rows.
"""
import json
import os
import re
import sys
import unicodedata
import uuid

HERE = os.path.dirname(__file__)
DATA_PATH = os.path.join(
    HERE, "..", "data", "farms_with_categorized_products.patched.json"
)

# Deterministic farm ids so re-seeding updates rather than duplicates.
FARM_NAMESPACE = uuid.uuid5(uuid.NAMESPACE_URL, "https://farms.app/seed")
SEED_CREATED_AT = "2026-06-01T00:00:00Z"

# The 13 canonical groups: German key (the dataset's key) -> (English, slug).
# Slugs are the stable public identity the frontend keys its display on.
GROUPS = [
    ("Früchte", "Fruits", "fruits"),
    ("Gemüse", "Vegetables", "vegetables"),
    ("Milchprodukte", "Dairy", "dairy"),
    ("Fleisch und Geflügel", "Meat & poultry", "meat-poultry"),
    ("Verarbeitete und haltbare Produkte", "Preserves & processed", "preserves"),
    ("Honig und Süßstoffe", "Honey & sweeteners", "honey-sweeteners"),
    ("Getränke", "Drinks", "drinks"),
    ("Backwaren und Gebäck", "Bakery", "bakery"),
    ("Blumen und Pflanzen", "Flowers & plants", "flowers-plants"),
    ("Nüsse, Samen und Öle", "Nuts, seeds & oils", "nuts-oils"),
    ("Getreide und Cerealien", "Grains & cereals", "grains"),
    ("Fisch und Meeresfrüchte", "Fish & seafood", "fish-seafood"),
    ("Sonstiges", "Other", "other"),
]
GROUP_SLUG_BY_DE = {de: slug for de, _en, slug in GROUPS}

VALID_CANTONS = {
    "AG", "AI", "AR", "BE", "BL", "BS", "FR", "GE", "GL", "GR", "JU", "LU",
    "NE", "NW", "OW", "SG", "SH", "SO", "SZ", "TG", "TI", "UR", "VD", "VS",
    "ZG", "ZH",
}


def slugify(value: str) -> str:
    """ASCII, lowercase, hyphen-separated slug (ä→a, & dropped)."""
    value = unicodedata.normalize("NFKD", value)
    value = value.encode("ascii", "ignore").decode("ascii")
    value = re.sub(r"[^a-zA-Z0-9]+", "-", value).strip("-").lower()
    return value or "item"


def sql_str(value) -> str:
    """A single-quoted SQL string literal (or NULL)."""
    if value is None:
        return "NULL"
    return "'" + str(value).replace("'", "''") + "'"


def to_canton(address: dict) -> str:
    iso = (address or {}).get("ISO3166-2-lvl4", "") or ""
    code = iso.replace("CH-", "") if iso.startswith("CH-") else ""
    return code if code in VALID_CANTONS else ""


def to_address(location: dict) -> str:
    address = location.get("address") or {}
    road = address.get("road") or ""
    postcode = address.get("postcode") or ""
    city = location.get("city") or address.get("village") or ""
    parts = [road, (postcode + " " + city).strip()]
    return ", ".join(p for p in parts if p) or location.get("display_name") or "Unnamed"


def main() -> int:
    with open(DATA_PATH, encoding="utf-8") as fh:
        raw = json.load(fh)
    locations = raw["locations"] if isinstance(raw, dict) else raw

    # --- Build the product taxonomy (deduped, stable slugs) -----------------
    # product key_de -> (slug, name_en, group_slug)
    products: dict[str, tuple[str, str, str]] = {}
    used_slugs: set[str] = set()
    unknown_groups: set[str] = set()

    for loc in locations:
        for group_de, items in (loc.get("categorized_products") or {}).items():
            group_slug = GROUP_SLUG_BY_DE.get(group_de)
            if group_slug is None:
                unknown_groups.add(group_de)
                continue
            for item in items:
                key_de = item["de"] if isinstance(item, dict) else item
                name_en = item.get("en", key_de) if isinstance(item, dict) else item
                # Blank names become SQL NULL: empty string and NULL must not
                # diverge downstream (a Some("") defeats the frontend's
                # name_en ?? fallback and renders a blank label).
                name_en = (name_en or "").strip() or None
                if key_de in products:
                    continue
                # Slug from the English name when present, else the German key —
                # never slugify(None) (name_en is None for a blank translation).
                base = slugify(name_en or key_de)
                slug = base
                n = 2
                while slug in used_slugs:
                    slug = f"{base}-{n}"
                    n += 1
                used_slugs.add(slug)
                products[key_de] = (slug, name_en, group_slug)

    if unknown_groups:
        sys.stderr.write(
            f"WARNING: {len(unknown_groups)} unknown group(s) skipped: "
            f"{sorted(unknown_groups)}\n"
        )

    # --- Build farm rows + links --------------------------------------------
    farm_rows: list[str] = []
    farm_category_links: list[str] = []  # (farm_id, group_slug)
    farm_product_links: list[str] = []  # (farm_id, product_slug)
    skipped = 0
    # ~73 source records share a url_title; disambiguate by occurrence so every
    # record gets a distinct, deterministic id (stable across re-runs).
    seed_key_counts: dict[str, int] = {}

    for loc in locations:
        lat, lng = loc.get("lat"), loc.get("lng")
        if lat is None or lng is None:
            skipped += 1
            continue
        base_key = loc.get("url_title") or loc.get("title") or "farm"
        seen = seed_key_counts.get(base_key, 0)
        seed_key_counts[base_key] = seen + 1
        seed_key = base_key if seen == 0 else f"{base_key}#{seen}"
        farm_id = str(uuid.uuid5(FARM_NAMESPACE, seed_key))
        name = loc.get("title") or "Unnamed farm"
        address = to_address(loc)
        canton = to_canton(loc.get("address"))

        farm_rows.append(
            f"({sql_str(farm_id)}, {sql_str(name)}, {sql_str(address)}, "
            f"{sql_str(canton)}, POINT({float(lng)}, {float(lat)}), "
            f"{sql_str(SEED_CREATED_AT)})"
        )

        cats = loc.get("categorized_products") or {}
        for group_de, items in cats.items():
            group_slug = GROUP_SLUG_BY_DE.get(group_de)
            if group_slug is None:
                continue
            farm_category_links.append(f"({sql_str(farm_id)}, {sql_str(group_slug)})")
            for item in items:
                key_de = item["de"] if isinstance(item, dict) else item
                entry = products.get(key_de)
                if entry:
                    farm_product_links.append(
                        f"({sql_str(farm_id)}, {sql_str(entry[0])})"
                    )

    # --- Emit SQL -----------------------------------------------------------
    out = sys.stdout.write
    out("-- Generated by scripts/seed_products.py. Idempotent; safe to re-run.\n")
    out("-- Apply: psql \"$DATABASE_URL\" -f seed.sql  (then restart the app).\n")
    out("BEGIN;\n\n")

    out("-- Category groups.\n")
    cat_values = ",\n  ".join(
        f"({sql_str(de)}, {sql_str(slug)}, {i})"
        for i, (de, _en, slug) in enumerate(GROUPS)
    )
    out(
        "INSERT INTO product_categories (key_de, slug, display_order)\n"
        f"VALUES\n  {cat_values}\n"
        "ON CONFLICT (key_de) DO UPDATE\n"
        "  SET slug = EXCLUDED.slug, display_order = EXCLUDED.display_order;\n\n"
    )

    out(f"-- Products ({len(products)} granular).\n")
    prod_values = ",\n  ".join(
        f"({sql_str(group_slug)}, {sql_str(key_de)}, {sql_str(slug)}, {sql_str(name_en)})"
        for key_de, (slug, name_en, group_slug) in products.items()
    )
    out(
        "INSERT INTO products (category_id, key_de, slug, name_en)\n"
        "SELECT c.id, v.key_de, v.slug, v.name_en\n"
        f"FROM (VALUES\n  {prod_values}\n) AS v(group_slug, key_de, slug, name_en)\n"
        "JOIN product_categories c ON c.slug = v.group_slug\n"
        "ON CONFLICT (key_de) DO UPDATE\n"
        "  SET slug = EXCLUDED.slug, name_en = EXCLUDED.name_en,\n"
        "      category_id = EXCLUDED.category_id;\n\n"
    )

    out(f"-- Farms ({len(farm_rows)}).\n")
    out(
        "INSERT INTO farms (id, name, address, canton, coordinates, created_at)\n"
        "VALUES\n  " + ",\n  ".join(farm_rows) + "\n"
                                                 "ON CONFLICT (id) DO UPDATE\n"
                                                 "  SET name = EXCLUDED.name, address = EXCLUDED.address,\n"
                                                 "      canton = EXCLUDED.canton, coordinates = EXCLUDED.coordinates;\n\n"
    )

    out(f"-- Group-level memberships ({len(farm_category_links)}).\n")
    out(
        "INSERT INTO farm_categories (farm_id, category_id)\n"
        "SELECT v.farm_id::uuid, c.id\n"
        "FROM (VALUES\n  " + ",\n  ".join(farm_category_links) + "\n"
                                                                 ") AS v(farm_id, group_slug)\n"
                                                                 "JOIN product_categories c ON c.slug = v.group_slug\n"
                                                                 "ON CONFLICT DO NOTHING;\n\n"
    )

    out(f"-- Granular product links ({len(farm_product_links)}).\n")
    out(
        "INSERT INTO farm_products (farm_id, product_id)\n"
        "SELECT v.farm_id::uuid, p.id\n"
        "FROM (VALUES\n  " + ",\n  ".join(farm_product_links) + "\n"
                                                                ") AS v(farm_id, product_slug)\n"
                                                                "JOIN products p ON p.slug = v.product_slug\n"
                                                                "ON CONFLICT DO NOTHING;\n\n"
    )

    out("COMMIT;\n")

    sys.stderr.write(
        f"Seed SQL: {len(GROUPS)} categories, {len(products)} products, "
        f"{len(farm_rows)} farms, {len(farm_category_links)} category links, "
        f"{len(farm_product_links)} product links "
        f"({skipped} farms skipped for missing coordinates).\n"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
