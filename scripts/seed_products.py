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
        python3 scripts/seed_products.py                  # writes ./seed.sql
        python3 scripts/seed_products.py path/to/out.sql  # custom output path
        psql "$DATABASE_URL" -f seed.sql                  # or the custom output path to the sql file

    Then RESTART the app so its boot-time taxonomy snapshot picks up the rows.
"""
import argparse
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

# The 13 canonical groups, with their display labels.
#
# (German key, English, slug, French, Italian, Romansh). The German key is the
# dataset's own key and doubles as the canonical German label; the slug is the
# stable public identity that filtering and the frontend key on.
#
# The fr/it/rm labels are authored translations carried over from the web
# frontend, which held them before the API did. They live here because this
# script owns the taxonomy's content — a migration cannot seed them, since
# migrations run before this does and would update an empty table.
GROUPS = [
    ('Früchte', 'Fruits', 'fruits', 'Fruits', 'Frutta', 'Fritgs'),
    ('Gemüse', 'Vegetables', 'vegetables', 'Légumes', 'Verdura', 'Verduras'),
    ('Milchprodukte', 'Dairy', 'dairy', 'Produits laitiers', 'Latticini', 'Products da latg'),
    ('Fleisch und Geflügel', 'Meat & poultry', 'meat-poultry', 'Viande et volaille', 'Carne e pollame', 'Charn e pulam'),
    ('Verarbeitete und haltbare Produkte', 'Preserves & processed', 'preserves', 'Produits transformés et conserves', 'Prodotti trasformati e conserve', 'Products transformads e conservads'),
    ('Honig und Süßstoffe', 'Honey & sweeteners', 'honey-sweeteners', 'Miel et édulcorants', 'Miele e dolcificanti', 'Mel e dultschadiras'),
    ('Getränke', 'Drinks', 'drinks', 'Boissons', 'Bevande', 'Bavrondas'),
    ('Backwaren und Gebäck', 'Bakery', 'bakery', 'Boulangerie', 'Prodotti da forno', 'Ar da furnaria'),
    ('Blumen und Pflanzen', 'Flowers & plants', 'flowers-plants', 'Fleurs et plantes', 'Fiori e piante', 'Flurs e plantas'),
    ('Nüsse, Samen und Öle', 'Nuts, seeds & oils', 'nuts-oils', 'Noix, graines et huiles', 'Noci, semi e oli', 'Nuschs, sems ed ieli'),
    ('Getreide und Cerealien', 'Grains & cereals', 'grains', 'Céréales', 'Cereali', 'Cereals'),
    ('Fisch und Meeresfrüchte', 'Fish & seafood', 'fish-seafood', 'Poisson et fruits de mer', 'Pesce e frutti di mare', 'Pesch e fritgs da mar'),
    ('Sonstiges', 'Other', 'other', 'Autres', 'Altro', 'Auter'),
]
GROUP_SLUG_BY_DE = {row[0]: row[2] for row in GROUPS}

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


def normalise_label(value) -> str | None:
    """A trimmed label, or None when there is nothing to display.

    Blank and NULL must not diverge: the database rejects blank labels, and a
    present-but-empty one would report itself as a translation and then render
    as nothing.
    """
    return (value or "").strip() or None


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
    parser = argparse.ArgumentParser(
        description="Generate idempotent seed SQL for the product taxonomy + farms."
    )
    parser.add_argument(
        "output",
        nargs="?",
        default="seed.sql",
        help="Path to write the generated SQL to (default: ./seed.sql).",
    )
    args = parser.parse_args()

    with open(DATA_PATH, encoding="utf-8") as fh:
        raw = json.load(fh)
    locations = raw["locations"] if isinstance(raw, dict) else raw

    # --- Build the product taxonomy (deduped, stable slugs) -----------------
    # product key_de -> (slug, group_slug, {lang: label})
    products: dict[str, tuple[str, str, dict[str, str | None]]] = {}
    used_slugs: set[str] = set()
    unknown_groups: set[str] = set()
    # A product's first group assignment wins; record any later record that
    # files the same key_de under a different group so data-quality issues
    # surface instead of being silently baked into the seed.
    group_conflicts: set[tuple[str, str, str]] = set()

    for loc in locations:
        for group_de, items in (loc.get("categorized_products") or {}).items():
            group_slug = GROUP_SLUG_BY_DE.get(group_de)
            if group_slug is None:
                unknown_groups.add(group_de)
                continue
            for item in items:
                key_de = item["de"] if isinstance(item, dict) else item
                # Every language the dataset may carry. Only de/en are populated
                # today (see #162); reading them all means a translation starts
                # flowing the moment it is authored, with no code change here.
                labels = {
                    lang: normalise_label(
                        item.get(lang) if isinstance(item, dict) else None
                    )
                    for lang in ("en", "fr", "it", "rm")
                }
                if key_de in products:
                    existing_group = products[key_de][1]
                    if existing_group != group_slug:
                        group_conflicts.add((key_de, existing_group, group_slug))
                    continue
                # Slug from the English name when present, else the German key.
                # Deliberately unchanged from before these columns existed:
                # slugs are the public, stable identity of a product, so they
                # must not move when a translation is added or removed.
                base = slugify(labels["en"] or key_de)
                slug = base
                n = 2
                while slug in used_slugs:
                    slug = f"{base}-{n}"
                    n += 1
                used_slugs.add(slug)
                products[key_de] = (slug, group_slug, labels)

    if unknown_groups:
        sys.stderr.write(
            f"WARNING: {len(unknown_groups)} unknown group(s) skipped: "
            f"{sorted(unknown_groups)}\n"
        )

    if group_conflicts:
        sys.stderr.write(
            f"WARNING: {len(group_conflicts)} product(s) seen under multiple "
            f"groups (kept the first): {sorted(group_conflicts)}\n"
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
    output_path = args.output
    with open(output_path, "w", encoding="utf-8") as fh_out:
        out = fh_out.write
        out("BEGIN;\n\n")

        out("-- Category groups.\n")
        cat_values = ",\n  ".join(
            f"({sql_str(de)}, {sql_str(slug)}, {i}, {sql_str(en)}, "
            f"{sql_str(fr)}, {sql_str(it)}, {sql_str(rm)})"
            for i, (de, en, slug, fr, it, rm) in enumerate(GROUPS)
        )
        out(
            "INSERT INTO product_categories\n"
            "  (key_de, slug, display_order, name_en, name_fr, name_it, name_rm)\n"
            f"VALUES\n  {cat_values}\n"
            "ON CONFLICT (key_de) DO UPDATE\n"
            "  SET slug = EXCLUDED.slug, display_order = EXCLUDED.display_order,\n"
            "      name_en = EXCLUDED.name_en, name_fr = EXCLUDED.name_fr,\n"
            "      name_it = EXCLUDED.name_it, name_rm = EXCLUDED.name_rm;\n\n"
        )

        out(f"-- Products ({len(products)} granular).\n")
        prod_values = ",\n  ".join(
            "("
            + ", ".join(
                sql_str(v)
                for v in (
                    group_slug,
                    key_de,
                    slug,
                    labels["en"],
                    labels["fr"],
                    labels["it"],
                    labels["rm"],
                )
            )
            + ")"
            for key_de, (slug, group_slug, labels) in products.items()
        )
        out(
            "INSERT INTO products\n"
            "  (category_id, key_de, slug, name_en, name_fr, name_it, name_rm)\n"
            "SELECT c.id, v.key_de, v.slug,\n"
            "       v.name_en, v.name_fr, v.name_it, v.name_rm\n"
            f"FROM (VALUES\n  {prod_values}\n"
            ") AS v(group_slug, key_de, slug, name_en, name_fr, name_it, name_rm)\n"
            "JOIN product_categories c ON c.slug = v.group_slug\n"
            "ON CONFLICT (key_de) DO UPDATE\n"
            "  SET slug = EXCLUDED.slug, name_en = EXCLUDED.name_en,\n"
            "      name_fr = EXCLUDED.name_fr, name_it = EXCLUDED.name_it,\n"
            "      name_rm = EXCLUDED.name_rm,\n"
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
        f"Seed SQL written to {output_path}: {len(GROUPS)} categories, "
        f"{len(products)} products, {len(farm_rows)} farms, "
        f"{len(farm_category_links)} category links, "
        f"{len(farm_product_links)} product links "
        f"({skipped} farms skipped for missing coordinates).\n"
    )
    sys.stderr.write("Generated by scripts/seed_products.py. Idempotent; safe to re-run.\n")
    sys.stderr.write(f"Apply: psql \"$DATABASE_URL\" -f {output_path}  (then restart the app).\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
