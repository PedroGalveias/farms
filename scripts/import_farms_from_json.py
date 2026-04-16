#!/usr/bin/env python3
"""
Import Swiss farm data from a JSON dataset into the `farms` PostgreSQL table.

The dataset is expected to contain a top-level `locations` array like the file
provided by the user. Each location is normalized into the current farms schema:

- `title` -> `name`
- address components -> `address`
- `ISO3166-2-lvl4` / `state` -> `canton`
- `lat` / `lng` -> `coordinates`
- `categorized_products` keys, with a `products` fallback -> `categories`

Rows missing required data after normalization are skipped and reported.
Duplicate farms are skipped both within the source file and against rows that
already exist in the database.
"""

from __future__ import annotations

import argparse
import html
import json
import os
import re
import sys
import uuid
from collections import Counter
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable
from urllib.parse import urlparse

try:
    import dotenv
except ImportError:  # pragma: no cover - optional local convenience dependency
    dotenv = None

DEFAULT_JSON_PATH = Path("/Users/pedro/Downloads/farms_with_categorized_products.json")
DEFAULT_DB_HOST = "127.0.0.1"
DEFAULT_DB_PORT = 5432
DEFAULT_DB_NAME = "farms"
DEFAULT_DB_USER = "postgres"
DEFAULT_DB_PASSWORD = "password"

NAME_FORBIDDEN_CHARACTERS = "/()\"<>\\{}"
# Order matters: these keys are tried from the most specific locality labels
# toward broader ones until we find the best available locality for the farm.
CITY_KEYS = [
    "village",
    "town",
    "city",
    "municipality",
    "hamlet",
    "isolated_dwelling",
    "locality",
    "suburb",
    "quarter",
    "neighbourhood",
]
CANTON_BY_STATE = {
    "Aargau": "AG",
    "Appenzell Ausserrhoden": "AR",
    "Appenzell Innerrhoden": "AI",
    "Basel-Landschaft": "BL",
    "Basel-Stadt": "BS",
    "Bern/Berne": "BE",
    "Fribourg/Freiburg": "FR",
    "Geneva": "GE",
    "Genève": "GE",
    "Glarus": "GL",
    "Graubünden/Grischun/Grigioni": "GR",
    "Jura": "JU",
    "Luzern": "LU",
    "Neuchâtel": "NE",
    "Nidwalden": "NW",
    "Obwalden": "OW",
    "Schaffhausen": "SH",
    "Schwyz": "SZ",
    "Solothurn": "SO",
    "St. Gallen": "SG",
    "Thurgau": "TG",
    "Ticino": "TI",
    "Uri": "UR",
    "Valais/Wallis": "VS",
    "Vaud": "VD",
    "Zug": "ZG",
    "Zürich": "ZH",
}
SWISS_COUNTRY_LABELS = frozenset({"schweiz", "suisse", "svizzera", "svizra"})


@dataclass(frozen=True)
class FarmRecord:
    source_index: int
    source_title: str
    name: str
    address: str
    canton: str
    latitude: float
    longitude: float
    categories: tuple[str, ...]

    @property
    def natural_key(self) -> tuple[str, str, str, float, float]:
        # This is the importer-side identity for a farm. We use it to detect
        # duplicates both within the source file and against rows already
        # present in the database, without relying on the source dataset's ids.
        return (
            self.name.casefold(),
            self.address.casefold(),
            self.canton,
            round(self.latitude, 6),
            round(self.longitude, 6),
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Import farms from a JSON dataset into the farms PostgreSQL table."
    )
    parser.add_argument(
        "--json-path",
        type=Path,
        default=DEFAULT_JSON_PATH,
        help=f"Path to the source JSON file (default: {DEFAULT_JSON_PATH})",
    )
    parser.add_argument(
        "--database-url",
        default=None,
        help=(
            "PostgreSQL connection string. Takes precedence over the split "
            "database flags when provided explicitly."
        ),
    )
    parser.add_argument(
        "--db-host",
        default=None,
        help=(
            "Database host for the split connection settings. When any split "
            "database flag is provided, the script builds the DSN from the split "
            f"settings instead of DATABASE_URL (default host: {DEFAULT_DB_HOST})."
        ),
    )
    parser.add_argument(
        "--db-port",
        type=int,
        default=None,
        help=f"Database port for the split connection settings (default: {DEFAULT_DB_PORT}).",
    )
    parser.add_argument(
        "--db-name",
        default=None,
        help=f"Database name for the split connection settings (default: {DEFAULT_DB_NAME}).",
    )
    parser.add_argument(
        "--db-user",
        default=None,
        help=f"Database user for the split connection settings (default: {DEFAULT_DB_USER}).",
    )
    parser.add_argument(
        "--db-password",
        default=None,
        help="Database password for the split connection settings.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Parse and validate the dataset without inserting rows.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        help="Only process the first N source rows after loading the JSON file.",
    )
    parser.add_argument(
        "--skip-example-limit",
        type=int,
        default=5,
        help="How many skipped-row examples to print per skip reason (default: 5).",
    )
    return parser.parse_args()


def load_dotenv() -> None:
    if dotenv is None:
        return

    dotenv_file = dotenv.find_dotenv(usecwd=True)
    if dotenv_file:
        dotenv.load_dotenv(dotenv_file)


def has_explicit_split_db_settings(args: argparse.Namespace) -> bool:
    # If any split DB flag is provided on the command line, prefer composing the
    # DSN from the split settings rather than silently falling back to
    # DATABASE_URL from the shell/.env file.
    return any(
        value is not None
        for value in (
            args.db_host,
            args.db_port,
            args.db_name,
            args.db_user,
            args.db_password,
        )
    )


def resolve_db_setting(explicit_value: str | None, env_var_name: str, default_value: str) -> str:
    if explicit_value is not None:
        return explicit_value

    env_value = os.getenv(env_var_name)
    if env_value is not None:
        return env_value

    return default_value


def resolve_db_port(explicit_port: int | None) -> int:
    if explicit_port is not None:
        return explicit_port

    env_port = os.getenv("POSTGRES_PORT")
    if env_port is None:
        return DEFAULT_DB_PORT

    try:
        return int(env_port)
    except ValueError as exc:
        raise ValueError("POSTGRES_PORT must be an integer.") from exc


def normalize_text(value: object | None) -> str | None:
    if value is None:
        return None
    # Source data mixes HTML entities, non-breaking spaces, and irregular
    # whitespace. Normalize them once here so all later parsing operates on
    # comparable strings.
    text = html.unescape(str(value))
    text = text.replace("\u00a0", " ")
    text = re.sub(r"\s+", " ", text).strip()
    return text or None


def sanitize_name(raw_name: object | None) -> str | None:
    text = normalize_text(raw_name)
    if text is None:
        return None

    # Farm names in the API/domain reject a few problematic characters. Replace
    # them up front so imported rows are shaped like rows created through the
    # Rust service.
    translation = str.maketrans({character: " " for character in NAME_FORBIDDEN_CHARACTERS})
    sanitized = text.translate(translation)
    sanitized = re.sub(r"\s+", " ", sanitized).strip()
    if not sanitized:
        return None
    return sanitized[:256]


def strip_trailing_swiss_country(display_name: str) -> str:
    # Nominatim-style display names often end with the multilingual Swiss
    # country label. Dropping it keeps fallback addresses shorter and more
    # consistent with addresses already stored in the service.
    parts = [part.strip() for part in display_name.split(",")]
    if not parts:
        return display_name.strip(" ,")

    trailing_part = parts[-1]
    trailing_labels = [label.strip().casefold() for label in trailing_part.split("/") if label.strip()]

    if trailing_labels and all(label in SWISS_COUNTRY_LABELS for label in trailing_labels):
        parts = parts[:-1]

    return ", ".join(part for part in parts if part).strip(" ,")


def build_address(location: dict) -> str | None:
    address = location.get("address") or {}
    display_name = normalize_text(location.get("display_name"))
    city = normalize_text(location.get("city"))

    road = normalize_text(address.get("road"))
    house_number = normalize_text(address.get("house_number"))
    street = " ".join(part for part in [road, house_number] if part)

    locality = None
    for key in CITY_KEYS:
        locality = normalize_text(address.get(key))
        if locality:
            break
    if locality is None:
        locality = city

    postcode = normalize_text(address.get("postcode"))

    segments: list[str] = []
    if street:
        segments.append(street)
    else:
        farm_name = normalize_text(address.get("farm"))
        if farm_name:
            segments.append(farm_name)

    locality_segment = " ".join(part for part in [postcode, locality] if part)
    if locality_segment:
        segments.append(locality_segment)

    candidate = ", ".join(segments)
    if len(candidate) >= 5:
        return candidate[:200]

    if display_name:
        # If the structured address is too sparse, fall back to the provider's
        # display name after trimming the trailing country label.
        cleaned_display_name = strip_trailing_swiss_country(display_name)
        if len(cleaned_display_name) >= 5:
            return cleaned_display_name[:200]

    return None


def extract_canton(location: dict) -> str | None:
    address = location.get("address") or {}
    iso_code = normalize_text(address.get("ISO3166-2-lvl4"))
    # Prefer ISO region codes when available because they are already explicit
    # and unambiguous (e.g. CH-ZH -> ZH).
    if iso_code and re.fullmatch(r"CH-[A-Z]{2}", iso_code):
        return iso_code.split("-", 1)[1]

    state = normalize_text(address.get("state"))
    # Fall back to matching the human-readable canton label from the source
    # against the set of canton spellings we support.
    if state and state in CANTON_BY_STATE:
        return CANTON_BY_STATE[state]

    return None


def extract_categories(location: dict) -> tuple[str, ...]:
    deduped: list[str] = []
    seen: set[str] = set()

    candidate_values: Iterable[object]
    categorized_products = location.get("categorized_products") or {}
    # The categorized view is the most useful signal. If it is missing, fall
    # back to the raw products list and use those values as categories.
    candidate_values = categorized_products.keys() if categorized_products else (location.get("products") or [])

    for value in candidate_values:
        category = normalize_text(value)
        if category is None:
            continue
        folded = category.casefold()
        if folded in seen:
            continue
        seen.add(folded)
        deduped.append(category[:50])
        if len(deduped) == 50:
            break

    return tuple(deduped)


def normalize_coordinates(location: dict) -> tuple[float, float] | None:
    try:
        latitude = round(float(location["lat"]), 6)
        longitude = round(float(location["lng"]), 6)
    except (KeyError, TypeError, ValueError):
        return None

    # Keep imports aligned with the current service validation rules by only
    # accepting coordinates inside Switzerland's bounding box.
    if not (45.8 <= latitude <= 47.9):
        return None
    if not (5.9 <= longitude <= 10.6):
        return None

    return latitude, longitude


def location_to_record(location: dict, source_index: int) -> tuple[FarmRecord | None, str | None]:
    # Convert one raw JSON location into the normalized record we can insert
    # into `farms`. Returning `(None, reason)` lets the caller aggregate skip
    # counts without raising for ordinary data-quality issues.
    name = sanitize_name(location.get("title") or location.get("display_name"))
    if name is None:
        return None, "missing_name"

    address = build_address(location)
    if address is None:
        return None, "missing_address"

    canton = extract_canton(location)
    if canton is None:
        return None, "missing_canton"

    coordinates = normalize_coordinates(location)
    if coordinates is None:
        return None, "invalid_coordinates"

    categories = extract_categories(location)
    if not categories:
        return None, "missing_categories"

    return (
        FarmRecord(
            source_index=source_index,
            source_title=normalize_text(location.get("title")) or f"row-{source_index}",
            name=name,
            address=address,
            canton=canton,
            latitude=coordinates[0],
            longitude=coordinates[1],
            categories=categories,
        ),
        None,
    )


def load_locations(path: Path) -> list[dict]:
    with path.open("r", encoding="utf-8") as source_file:
        payload = json.load(source_file)

    if not isinstance(payload, dict):
        raise ValueError("Expected the JSON file to contain a top-level object.")

    locations = payload.get("locations")
    if not isinstance(locations, list):
        raise ValueError("Expected the top-level object to contain a `locations` array.")

    return locations


def build_connection_dsn(args: argparse.Namespace) -> str:
    # Connection precedence:
    # 1. Explicit --database-url
    # 2. Explicit split DB flags (--db-host/--db-port/...)
    # 3. DATABASE_URL from the environment/.env
    # 4. Split DB env vars, then built-in defaults
    if args.database_url:
        return args.database_url

    if not has_explicit_split_db_settings(args):
        database_url = os.getenv("DATABASE_URL")
        if database_url:
            return database_url

    db_host = resolve_db_setting(args.db_host, "POSTGRES_HOST", DEFAULT_DB_HOST)
    db_port = resolve_db_port(args.db_port)
    db_name = resolve_db_setting(args.db_name, "POSTGRES_DB", DEFAULT_DB_NAME)
    db_user = resolve_db_setting(args.db_user, "POSTGRES_USER", DEFAULT_DB_USER)
    db_password = resolve_db_setting(args.db_password, "POSTGRES_PASSWORD", DEFAULT_DB_PASSWORD)

    return (
        f"postgresql://{db_user}:{db_password}"
        f"@{db_host}:{db_port}/{db_name}"
    )


def redact_dsn(dsn: str) -> str:
    parsed = urlparse(dsn)
    if not parsed.scheme or not parsed.hostname:
        return dsn
    username = parsed.username or ""
    password_mask = ":***" if parsed.password else ""
    auth = f"{username}{password_mask}@" if username or password_mask else ""
    port = f":{parsed.port}" if parsed.port else ""
    database = parsed.path or ""
    return f"{parsed.scheme}://{auth}{parsed.hostname}{port}{database}"


def parse_point_text(point_text: str) -> tuple[float, float]:
    inner = point_text.strip().removeprefix("(").removesuffix(")")
    longitude_text, latitude_text = inner.split(",", 1)
    return round(float(latitude_text), 6), round(float(longitude_text), 6)


def fetch_existing_natural_keys(connection) -> set[tuple[str, str, str, float, float]]:
    with connection.cursor() as cursor:
        cursor.execute(
            """
            SELECT name,
                   address,
                   canton,
                   coordinates::text AS coordinates_text
            FROM farms
            """
        )
        rows = cursor.fetchall()

    existing_keys: set[tuple[str, str, str, float, float]] = set()
    for name, address, canton, coordinates_text in rows:
        latitude, longitude = parse_point_text(coordinates_text)
        # Mirror `FarmRecord.natural_key` so the importer can compare incoming
        # rows to already-imported rows using the same duplicate logic.
        existing_keys.add(
            (
                str(name).casefold(),
                str(address).casefold(),
                str(canton),
                latitude,
                longitude,
            )
        )
    return existing_keys


def insert_records(connection, records: list[FarmRecord]) -> None:
    from psycopg2.extras import execute_batch

    now = datetime.now(timezone.utc)
    # Generate database rows in the order expected by the INSERT statement.
    # Coordinates are stored as PostgreSQL POINT(longitude, latitude).
    rows = [
        (
            str(uuid.uuid4()),
            record.name,
            record.address,
            record.canton,
            record.longitude,
            record.latitude,
            list(record.categories),
            now,
            None,
        )
        for record in records
    ]

    with connection.cursor() as cursor:
        execute_batch(
            cursor,
            """
            INSERT INTO farms (id,
                               name,
                               address,
                               canton,
                               coordinates,
                               categories,
                               created_at,
                               updated_at)
            VALUES (%s,
                    %s,
                    %s,
                    %s,
                    POINT(%s, %s),
                    %s,
                    %s,
                    %s)
            """,
            rows,
            page_size=200,
        )


def main() -> int:
    load_dotenv()
    args = parse_args()

    if not args.json_path.exists():
        print(f"JSON file not found: {args.json_path}", file=sys.stderr)
        return 1

    try:
        locations = load_locations(args.json_path)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"Failed to load source dataset: {exc}", file=sys.stderr)
        return 1

    if args.limit is not None:
        locations = locations[: args.limit]

    skip_counts: Counter[str] = Counter()
    skip_examples: dict[str, list[str]] = {}
    seen_in_source: set[tuple[str, str, str, float, float]] = set()
    valid_records: list[FarmRecord] = []

    for source_index, location in enumerate(locations, start=1):
        record, skip_reason = location_to_record(location, source_index)
        if record is None:
            skip_counts[skip_reason or "unknown_skip_reason"] += 1
            examples = skip_examples.setdefault(skip_reason or "unknown_skip_reason", [])
            if len(examples) < args.skip_example_limit:
                title = normalize_text(location.get("title")) or f"row-{source_index}"
                examples.append(title)
            continue

        if record.natural_key in seen_in_source:
            skip_counts["duplicate_in_source"] += 1
            examples = skip_examples.setdefault("duplicate_in_source", [])
            if len(examples) < args.skip_example_limit:
                examples.append(record.source_title)
            continue

        # Only rows that survive normalization and in-file duplicate detection
        # move on to the database comparison/insert phase.
        seen_in_source.add(record.natural_key)
        valid_records.append(record)

    print(f"Loaded {len(locations)} source rows from {args.json_path}")
    print(f"Normalized {len(valid_records)} rows into valid farm records")
    for reason, count in skip_counts.most_common():
        print(f"Skipped {count:>4} rows due to {reason}")
        examples = skip_examples.get(reason) or []
        if examples:
            print(f"  Examples: {', '.join(examples)}")

    if args.dry_run:
        print("Dry run enabled; no database changes were made.")
        return 0

    try:
        dsn = build_connection_dsn(args)
    except ValueError as exc:
        print(f"Invalid database configuration: {exc}", file=sys.stderr)
        return 1

    print("Connecting to database...")

    try:
        import psycopg2
    except ImportError as exc:
        print(
            "Database import requires `psycopg2-binary`. Install dependencies with "
            "`pip install -r requirements.txt` and rerun the script.",
            file=sys.stderr,
        )
        return 1

    try:
        with psycopg2.connect(dsn) as connection:
            with connection.cursor() as cursor:
                cursor.execute("SELECT to_regclass('public.farms')")
                table_name = cursor.fetchone()[0]
                if table_name != "farms":
                    print("Table `public.farms` was not found. Run migrations first.", file=sys.stderr)
                    return 1

            existing_keys = fetch_existing_natural_keys(connection)
            # Compare normalized incoming rows to existing farms using the same
            # natural-key logic as the in-memory source deduplication.
            new_records = [record for record in valid_records if record.natural_key not in existing_keys]
            duplicate_in_db = len(valid_records) - len(new_records)

            print(f"Found {len(existing_keys)} existing farms in the database")
            print(f"Skipping {duplicate_in_db} rows already present in the database")

            if not new_records:
                print("No new farms to insert.")
                return 0

            insert_records(connection, new_records)
            print(f"Inserted {len(new_records)} farm rows into `farms`.")
            return 0
    except psycopg2.Error as exc:
        print(f"Database import failed: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
