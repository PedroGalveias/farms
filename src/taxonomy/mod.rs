//! In-memory snapshot of the product taxonomy.
//!
//! Loaded once at startup and shared via `web::Data`. It resolves a product
//! `slug` to its numeric `id` without a database round trip (giving early 400s
//! for unknown slugs) and looks products up by id.
//!
//! The taxonomy tables are populated out-of-band (a separate seeding step), so
//! this snapshot may be empty until seeding has run and the app has been
//! (re)started.

use sqlx::PgPool;
use std::collections::HashMap;

/// A single product plus the group it belongs to.
#[derive(Clone)]
pub struct ProductInfo {
    pub id: i32,
    pub slug: String,
    pub name_en: Option<String>,
    pub group_slug: String,
}

/// Read-mostly, in-process index of the product taxonomy.
#[derive(Clone, Default)]
pub struct TaxonomySnapshot {
    by_slug: HashMap<String, ProductInfo>,
    by_id: HashMap<i32, ProductInfo>,
    /// Category (group) slug -> id, for validating category filters and
    /// resolving group-level farm associations.
    category_by_slug: HashMap<String, i16>,
}

impl TaxonomySnapshot {
    /// Load the whole taxonomy from Postgres. Cheap: at most a few hundred rows.
    #[tracing::instrument(name = "Load taxonomy snapshot", skip(pool))]
    pub async fn load(pool: &PgPool) -> Result<Self, anyhow::Error> {
        let rows = sqlx::query!(
            r#"
            SELECT
                p.id,
                p.slug,
                p.name_en,
                c.slug AS "group_slug!"
            FROM products p
            JOIN product_categories c ON c.id = p.category_id
            "#
        )
        .fetch_all(pool)
        .await?;

        let mut by_slug = HashMap::with_capacity(rows.len());
        let mut by_id = HashMap::with_capacity(rows.len());
        for row in rows {
            let info = ProductInfo {
                id: row.id,
                slug: row.slug.clone(),
                name_en: row.name_en,
                group_slug: row.group_slug,
            };
            by_slug.insert(row.slug, info.clone());
            by_id.insert(row.id, info);
        }

        let category_rows = sqlx::query!(r#"SELECT id, slug FROM product_categories"#)
            .fetch_all(pool)
            .await?;
        let mut category_by_slug = HashMap::with_capacity(category_rows.len());
        for row in category_rows {
            category_by_slug.insert(row.slug, row.id);
        }

        Ok(Self {
            by_slug,
            by_id,
            category_by_slug,
        })
    }

    /// Resolve a slug to a product id, or `None` if the slug is unknown.
    pub fn id_for_slug(&self, slug: &str) -> Option<i32> {
        self.by_slug.get(slug).map(|p| p.id)
    }

    /// Resolve a category (group) slug to its id, or `None` if unknown.
    pub fn category_id_for_slug(&self, slug: &str) -> Option<i16> {
        self.category_by_slug.get(slug).copied()
    }

    /// Look up a product by id.
    #[allow(dead_code)]
    pub fn by_id(&self, id: i32) -> Option<&ProductInfo> {
        self.by_id.get(&id)
    }

    /// Number of products in the snapshot (useful for a startup log).
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}
