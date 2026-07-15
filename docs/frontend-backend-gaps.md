# Frontend ⇄ Backend gaps

What the frontend (`farms-frontend`) already has or expects that the backend
does not yet provide. Written 2026-07-15, after the sub-categories work landed
(taxonomy, products/categories model, moderation, seeding).

Priority: **P1** blocks a signature frontend feature · **P2** real feature gap ·
**P3** polish / quick win.

---

## 1. Contract mismatches (frontend must adapt — backend is source of truth)

These are consequences of the sub-categories refactor. Listed so the frontend
migration is planned, not discovered in production.

| What changed                                                            | Frontend today                                                    | Action                                                                           |
|-------------------------------------------------------------------------|-------------------------------------------------------------------|----------------------------------------------------------------------------------|
| `GET /farms` now returns `{ farms: [...], next_cursor }`                | parses a bare `Farm[]` (`getFarms()` does `res.json() as Farm[]`) | read `.farms`, thread `next_cursor`                                              |
| `categories` values are now group **slugs** (`"fruits"`)                | category catalog keyed by German (`"Gemüse"`)                     | re-key catalog + all lookups to slugs (`product_categories.slug`)                |
| Each farm carries `products[]` (slug, group, status, last_confirmed_at) | no product-level data consumed                                    | consume `products[]` (powers the sub-category picker, the live map, seasonality) |
| `POST /farms` wants `products` and/or `categories` (slugs)              | sends `categories` (German strings)                               | send slugs; add-a-farm picks products/categories                                 |

These are frontend work, but they only settle once the two P1 items below are
decided (they change *how* the frontend adapts).

---

## 2. Missing backend capabilities

### P1 — Nearest-first (distance sort + radius)

The app's core ritual is *nearest farms first*, and the directory sorts by
distance. The backend paginates by `(created_at DESC, id DESC)` only and never
receives the user's location, so the frontend must still download **all** farms
(~264 kB at 240, prod ships 3,155) to sort client-side — negating server-side
pagination for the main flow.
**Recommendation:** accept `?lat=&lng=&radius_km=` and sort by distance
(PostGIS `earthdistance`/`geography <->`, or a bounding-box prefilter + haversine).
Until then, the frontend cannot move its hero/near-me flows server-side.

### P2 — Server-side canton filter

Directory + canton landing pages filter by canton; `/farms` has no `?canton=`.
Cheap to add (indexed text equality); pairs with pagination so a canton page
doesn't pull the whole dataset.

### P2 — Free-text search

The directory search box matches farm name / address / product. No `?q=` exists.
Options: Postgres `ILIKE`/trigram (`pg_trgm`) or full-text (`tsvector`).

### P2 — Product/category i18n

`ProductDto` exposes only `slug` + `name_en`; the app is 5-locale
(de/fr/it/rm/en). **Cleanest, matches today's category handling:** the API
exposes the stable **slug**, the frontend owns translations keyed by slug
(`name_en` stays a fallback). The seed already stores `key_de` (German) —
expose it too if the backend should own display strings instead.

### P2 — Seasonality months

`stock_status` is `AVAILABLE | SEASONAL | UNAVAILABLE` — a flag with no "which
months". The frontend has a rich seasonal calendar; to make it backend-driven,
`farm_products` (or `products`) needs per-item season months.

### P2 — `farm.photos`

`/farm/[id]` has a gallery template; `Farm.photos` is optional and unserved.
Backend has no photo storage/columns. Needs an images table + upload/storage
(e.g. object storage + signed URLs) — a self-contained feature.

### P3 — `GET /me` returns only `user_id` + `role`

The frontend wants to show the **username** (it currently falls back to the role,
"member"). `users.username` exists — add it to `MeResponse`. One-line quick win.

### P3 — Sort options (name, canton)

Directory offers newest / name / canton / nearest. Only `created_at DESC` is
server-side. `name`/`canton` are trivial `ORDER BY` additions (keyset needs a
matching composite for each); `nearest` is the P1 item.

### P3 — Opening hours

The **source dataset already has `opening_hours`** (sparse — ~189 records, many
empty), but the schema/API drop it. If hours are wanted, add a column and expose
it; the seeder can populate from the dataset.

### Future — Reviews

On the product roadmap; no backend model yet.

---

## 3. Data / seed gaps

- **897 farms (~28%) have no `categorized_products`** in the source, so the
  seeder gives them neither category nor product links — they appear in the
  directory but match no category/product filter (same as the frontend today).
  Their raw `products` German array is ungrouped; mapping it to known product
  keys could recover group membership for some. **P3.**
- **No "group-only" farms in this dataset:** every farm with categorized
  products has both category and product links. `farm_categories` is still
  needed for the group-level *add-a-farm / suggestion* write paths and for an
  index-efficient `?category=` filter, but this particular import doesn't
  exercise the coarse-only read case.

---

## 4. Minor / hardening

- **Duplicate pending suggestions** aren't prevented — a user can submit the
  same (farm, product, action) repeatedly. Add a unique partial index
  `(farm_id, product_id, submitted_by) WHERE status = 'PENDING'`. **P3.**
- **Suggestion submit isn't idempotent** (no idempotency key). Low risk;
  moderation dedupes. **P3.**
- **A user can't see their own submitted suggestions** (no read endpoint).
  Nice-to-have for a "your suggestions" view. **P3.**

---

## Suggested order

1. **P1 nearest-first** — unblocks the frontend's main flow and decides how the
   frontend adopts pagination.
2. **P2 canton filter + free-text search + i18n decision** — the rest of the
   directory/search surface.
3. **P3 quick wins** — `/me` username, duplicate-suggestion index, sort options.
4. **Features** — photos, seasonality months, reviews (each its own PR).
