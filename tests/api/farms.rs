use crate::helpers::{TestApp, TestUser, redis_exists_with_retry, seed_test_taxonomy, spawn_app};
use actix_web::http::StatusCode;
use chrono::{DateTime, Utc};
use deadpool_redis::redis::AsyncCommands;
use fake::{
    Fake,
    faker::{address::de_de::StreetName, name::de_de::Name as FakerName},
};
use farms::{
    configuration::IdempotencyEngine,
    domain::farm::{Address, Canton, Name, Point},
    idempotency::{ExpiryOutcome, HeaderPair, IdempotencyData, IdempotencyKey},
};
use rand::RngExt;
use std::ops::Sub;
use std::{collections::HashSet, ops::Add, time::Duration};
use uuid::Uuid;

/// A farm's own fields for tests — the taxonomy (categories/products) is a
/// separate dimension now, seeded and linked explicitly per test.
struct TestFarm {
    id: Uuid,
    name: Name,
    address: Address,
    canton: Canton,
    coordinates: Point,
    created_at: DateTime<Utc>,
}

/// Generate a valid Swiss coordinate within Switzerland boundaries.
fn generate_swiss_coordinates() -> String {
    let lat = 45.8 + (rand::random::<f64>() * (47.9 - 45.8));
    let lon = 5.9 + (rand::random::<f64>() * (10.6 - 5.9));
    format!("{:.4},{:.4}", lat, lon)
}

fn generate_swiss_canton() -> Canton {
    let cantons = vec![
        "ZH", "BE", "LU", "UR", "SZ", "OW", "NW", "GL", "ZG", "FR", "SO", "BS", "BL", "SH", "AR",
        "AI", "SG", "GR", "AG", "TG", "TI", "VD", "VS", "NE", "GE", "JU",
    ];

    let mut rng = rand::rng();
    let index = rng.random_range(0..cantons.len());
    Canton::parse(cantons[index].to_string()).expect("Generated invalid canton")
}

fn generate_farm() -> TestFarm {
    let id = Uuid::new_v4();
    let name = Name::parse(FakerName().fake()).expect("Generated invalid farm name");
    let address = Address::parse(StreetName().fake()).expect("Generated invalid address");
    let canton = generate_swiss_canton();
    let coordinates_str = generate_swiss_coordinates();
    let coordinates = Point::parse(&coordinates_str).expect("Generated invalid coordinates");
    let created_at = Utc::now();

    TestFarm {
        id,
        name,
        address,
        canton,
        coordinates,
        created_at,
    }
}

/// The create body for a farm. Every farm needs at least one classification;
/// these tests use the `strawberries` product (seed the taxonomy first).
fn farm_to_json(farm: &TestFarm, idempotency_key: Uuid) -> serde_json::Value {
    serde_json::json!({
        "name": farm.name,
        "address": farm.address,
        "canton": farm.canton,
        "coordinates": farm.coordinates,
        "products": ["strawberries"],
        "idempotency_key": idempotency_key.to_string(),
    })
}

/// Insert a farm row directly (no taxonomy links) for read-path tests.
async fn insert_farm_in_db(app: &TestApp, farm: &TestFarm) {
    sqlx::query!(
        r#"
        INSERT INTO farms (id, name, address, canton, coordinates, created_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        farm.id,
        &farm.name as &Name,
        &farm.address as &Address,
        &farm.canton as &Canton,
        &farm.coordinates as &Point,
        farm.created_at,
    )
    .execute(&app.db_pool)
    .await
    .expect("Failed to execute query");
}

async fn break_farms_table(app: &TestApp) {
    sqlx::query!("ALTER TABLE farms DROP COLUMN name;")
        .execute(&app.db_pool)
        .await
        .expect("Failed to execute query");
}

/// The `farms` array from a `GET /farms` list response (`{farms, next_cursor}`).
async fn farms_array(response: reqwest::Response) -> Vec<serde_json::Value> {
    let body: serde_json::Value = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");
    body["farms"]
        .as_array()
        .expect("Response is missing a `farms` array.")
        .clone()
}

async fn create_single_farm(app: &TestApp) -> TestFarm {
    let farm = generate_farm();
    insert_farm_in_db(app, &farm).await;
    farm
}

async fn create_n_farms(app: &TestApp, n: usize) -> Vec<TestFarm> {
    let mut farms = Vec::<TestFarm>::with_capacity(n);
    for _ in 0..n {
        farms.push(create_single_farm(app).await);
    }
    farms
}

async fn log_in_test_user(app: &TestApp, user: &TestUser) {
    user.store(&app.db_pool).await;

    let response = app
        .post_login(&serde_json::json!({
            "email": user.email,
            "password": user.password
        }))
        .await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn get_farms_returns_empty_list_when_no_farms_exist() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app.get_farms().await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
    assert_eq!(0, farms_array(response).await.len());
}

#[tokio::test]
async fn get_farms_returns_200_and_a_single_farm() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let created_farm = create_single_farm(&app).await;

    let response = app.get_farms().await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
    let farms = farms_array(response).await;

    assert_eq!(1, farms.len());
    assert_eq!(
        created_farm.name.as_str(),
        farms[0]["name"].as_str().unwrap()
    );
    assert_eq!(
        created_farm.canton.as_str(),
        farms[0]["canton"].as_str().unwrap()
    );
}

#[tokio::test]
async fn get_farms_returns_200_and_list_of_farms() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let created_farms = create_n_farms(&app, 10).await;

    let response = app.get_farms().await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
    let farms = farms_array(response).await;

    assert_eq!(created_farms.len(), farms.len());
    for created_farm in &created_farms {
        let id = created_farm.id.to_string();
        assert!(farms.iter().any(|f| f["id"].as_str() == Some(&id)));
    }
}

#[tokio::test]
async fn get_farms_returns_500_when_unexpected_error_occurs() {
    let app = spawn_app(IdempotencyEngine::None).await;
    break_farms_table(&app).await;

    let response = app.get_farms().await;

    assert_eq!(
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn get_farms_filters_by_category_including_group_only_farms() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let taxonomy = seed_test_taxonomy(&app.db_pool).await;

    // A farm classified only at the group level (Vegetables), no product.
    let group_only = crate::helpers::insert_test_farm(&app.db_pool, "Group only").await;
    crate::helpers::link_farm_category(&app.db_pool, group_only, taxonomy.vegetables_category_id)
        .await;

    // A farm with a granular product in a different group (Fruits).
    let granular = crate::helpers::insert_test_farm(&app.db_pool, "Granular").await;
    crate::helpers::link_farm_product(&app.db_pool, granular, taxonomy.strawberries_id).await;

    let response = app
        .api_client
        .get(format!("{}/farms?category=vegetables", app.address))
        .send()
        .await
        .expect("Failed to execute request.");
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let farms = farms_array(response).await;
    assert_eq!(1, farms.len());
    assert_eq!(group_only.to_string(), farms[0]["id"].as_str().unwrap());
    // The derived categories surface the group-level membership.
    let categories: Vec<&str> = farms[0]["categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(vec!["vegetables"], categories);
}

#[tokio::test]
async fn get_farm_returns_200_and_the_request_farm_when_it_exists() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let requested_farm = create_single_farm(&app).await;
    let other_farm = create_single_farm(&app).await;

    let response = app.get_farm(requested_farm.id).await;

    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());
    let farm: serde_json::Value = response.json().await.expect("Failed to parse JSON.");

    assert_eq!(requested_farm.id.to_string(), farm["id"].as_str().unwrap());
    assert_eq!(requested_farm.name.as_str(), farm["name"].as_str().unwrap());
    assert_eq!(
        requested_farm.address.as_str(),
        farm["address"].as_str().unwrap()
    );
    assert_eq!(
        requested_farm.canton.as_str(),
        farm["canton"].as_str().unwrap()
    );
    assert_ne!(other_farm.id.to_string(), farm["id"].as_str().unwrap());
}

#[tokio::test]
async fn get_farm_returns_404_when_farm_does_not_exist() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app.get_farm(Uuid::new_v4()).await;

    assert_eq!(StatusCode::NOT_FOUND.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn get_farm_returns_400_when_farm_id_is_not_an_uuid() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let response = app.get_farm_by_raw_id("not-a-valid-uuid").await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn get_farm_returns_500_when_unexpected_error_occurs() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let farm_id = Uuid::new_v4();
    break_farms_table(&app).await;

    let response = app.get_farm(farm_id).await;

    assert_eq!(
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn create_farm_returns_201_for_valid_body_data() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = farm_to_json(&farm, idempotency_key);
    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let saved = sqlx::query!(
        r#"
        SELECT name as "name: Name", address as "address: Address",
               canton as "canton: Canton", coordinates as "coordinates: Point"
        FROM farms
        "#
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved farm.");

    assert_eq!(saved.name, farm.name);
    assert_eq!(saved.address, farm.address);
    assert_eq!(saved.canton, farm.canton);
    assert_eq!(saved.coordinates, farm.coordinates);

    // The product link was persisted.
    let linked = sqlx::query!(
        r#"
        SELECT p.slug AS "slug!"
        FROM farm_products fp
        JOIN products p ON p.id = fp.product_id
        "#
    )
    .fetch_all(&app.db_pool)
    .await
    .expect("Failed to fetch farm products.");
    let slugs: Vec<String> = linked.into_iter().map(|r| r.slug).collect();
    assert_eq!(vec!["strawberries".to_string()], slugs);
}

#[tokio::test]
async fn create_farm_returns_201_for_authenticated_admins() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let user = TestUser::generate_admin();
    log_in_test_user(&app, &user).await;

    let farm = generate_farm();
    let body = farm_to_json(&farm, Uuid::new_v4());
    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_for_unknown_product() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Road 1, 8000 Zürich",
        "canton": "ZH",
        "coordinates": "47.3769,8.5417",
        "products": ["dragonfruit"],
        "idempotency_key": Uuid::new_v4().to_string(),
    });
    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_when_no_category_or_product_is_given() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Road 1, 8000 Zürich",
        "canton": "ZH",
        "coordinates": "47.3769,8.5417",
        "idempotency_key": Uuid::new_v4().to_string(),
    });
    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_accepts_group_only_classification() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Group Only Farm",
        "address": "Road 1, 8000 Zürich",
        "canton": "ZH",
        "coordinates": "47.3769,8.5417",
        "categories": ["vegetables"],
        "idempotency_key": Uuid::new_v4().to_string(),
    });
    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let linked = sqlx::query!(
        r#"
        SELECT c.slug AS "slug!"
        FROM farm_categories fc
        JOIN product_categories c ON c.id = fc.category_id
        "#
    )
    .fetch_all(&app.db_pool)
    .await
    .expect("Failed to fetch farm categories.");
    let slugs: Vec<String> = linked.into_iter().map(|r| r.slug).collect();
    assert_eq!(vec!["vegetables".to_string()], slugs);
}

#[tokio::test]
async fn create_farm_returns_a_500_when_unexpected_error_occurs() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    break_farms_table(&app).await;

    let body = farm_to_json(&farm, idempotency_key);
    let response = app.post_farm(&body).await;

    assert_eq!(
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn create_farm_returns_401_for_unauthenticated_users() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let farm = generate_farm();
    let body = farm_to_json(&farm, Uuid::new_v4());

    let response = app.post_farm(&body).await;

    assert_eq!(
        StatusCode::UNAUTHORIZED.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn create_farm_returns_a_400_for_invalid_body_data() {
    let app = spawn_app(IdempotencyEngine::None).await;
    let test_cases = vec![
        (
            serde_json::json!({
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "ZH",
                "coordinates": "47.3769,8.5417",
                "products": ["strawberries"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'name'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "canton": "ZH",
                "coordinates": "47.3769,8.5417",
                "products": ["strawberries"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'address'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "coordinates": "47.3769,8.5417",
                "products": ["strawberries"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'canton'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "ZH",
                "products": ["strawberries"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'coordinates'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "ZH",
                "coordinates": "47.3769,8.5417",
                "products": ["strawberries"],
            }),
            "missing field 'idempotency_key'",
        ),
    ];

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    for (invalid_body, error_message) in test_cases {
        let response = app.post_farm(&invalid_body).await;
        assert_eq!(
            StatusCode::BAD_REQUEST.as_u16(),
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request when the payload was {}.",
            error_message
        );
    }
}

#[tokio::test]
async fn create_farm_returns_400_for_invalid_coordinate_format() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let test_cases = vec![
        ("invalid", "invalid coordinate format"),
        ("47.3769", "single number coordinate"),
        ("abc,def", "non-numeric coordinates"),
    ];

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    for (coordinates, error_message) in test_cases {
        let body = serde_json::json!({
            "name": "Test Farm",
            "address": "Test Address",
            "canton": "ZH",
            "coordinates": coordinates,
            "products": ["strawberries"],
            "idempotency_key": Uuid::new_v4(),
        });
        let response = app.post_farm(&body).await;
        assert_eq!(
            StatusCode::BAD_REQUEST.as_u16(),
            response.status().as_u16(),
            "The API did not fail with 400 Bad Request for {}.",
            error_message
        );
    }
}

#[tokio::test]
async fn create_farm_returns_400_for_coordinates_outside_switzerland() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Berlin Farm",
        "address": "Berlin Street 1",
        "canton": "ZH",
        "coordinates": "52.5200,13.4050",
        "products": ["strawberries"],
        "idempotency_key": Uuid::new_v4(),
    });

    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_for_invalid_latitude() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Test Address",
        "canton": "ZH",
        "coordinates": "91.0,8.5417",
        "products": ["strawberries"],
        "idempotency_key": Uuid::new_v4(),
    });

    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_for_invalid_longitude() {
    let app = spawn_app(IdempotencyEngine::None).await;

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Test Address",
        "canton": "ZH",
        "coordinates": "47.3769,181.0",
        "products": ["strawberries"],
        "idempotency_key": Uuid::new_v4(),
    });

    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_201_for_all_valid_swiss_cantons() {
    let app = spawn_app(IdempotencyEngine::None).await;
    seed_test_taxonomy(&app.db_pool).await;

    let cantons = vec![
        "ZH", "BE", "LU", "UR", "SZ", "OW", "NW", "GL", "ZG", "FR", "SO", "BS", "BL", "SH", "AR",
        "AI", "SG", "GR", "AG", "TG", "TI", "VD", "VS", "NE", "GE", "JU",
    ];

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    for canton in cantons {
        let body = serde_json::json!({
            "name": format!("{} Farm", canton),
            "address": "Test Address",
            "canton": canton,
            "coordinates": "47.3769,8.5417",
            "products": ["strawberries"],
            "idempotency_key": Uuid::new_v4(),
        });

        let response = app.post_farm(&body).await;
        assert_eq!(
            StatusCode::CREATED.as_u16(),
            response.status().as_u16(),
            "Failed to create farm for canton {}",
            canton
        );
    }
}

#[tokio::test]
async fn create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db_redis()
{
    create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db(
        IdempotencyEngine::Redis,
    )
    .await;
}

#[tokio::test]
async fn create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db_postgres()
 {
    create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db(
        IdempotencyEngine::Postgres,
    )
    .await;
}

async fn create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db(
    idempotency_engine: IdempotencyEngine,
) {
    let app = spawn_app(idempotency_engine).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = farm_to_json(&farm, idempotency_key);

    let response1 = app.post_farm(&body).await;
    let response2 = app.post_farm(&body).await;

    assert_eq!(response1.status(), StatusCode::CREATED.as_u16());
    assert_eq!(response2.status(), StatusCode::CREATED.as_u16());

    let saved = sqlx::query!("SELECT id FROM farms")
        .fetch_all(&app.db_pool)
        .await
        .expect("Failed to fetch saved farms.");

    assert_eq!(saved.len(), 1);
}

#[tokio::test]
async fn create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db_redis() {
    create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db(
        IdempotencyEngine::Redis,
    )
    .await;
}

#[tokio::test]
async fn create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db_postgres()
 {
    create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db(
        IdempotencyEngine::Postgres,
    )
    .await;
}

async fn create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db(
    idempotency_engine: IdempotencyEngine,
) {
    let app = spawn_app(idempotency_engine).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = farm_to_json(&farm, idempotency_key);

    let response1 = app.post_farm(&body);
    let response2 = app.post_farm(&body);
    let (response1, response2) = tokio::join!(response1, response2);

    let status1 = response1.status().as_u16();
    let status2 = response2.status().as_u16();

    let allowed_statuses =
        HashSet::from([StatusCode::CREATED.as_u16(), StatusCode::CONFLICT.as_u16()]);

    assert!(allowed_statuses.contains(&status1));
    assert!(allowed_statuses.contains(&status2));
    assert!(status1 == StatusCode::CREATED.as_u16() || status2 == StatusCode::CREATED.as_u16());

    let saved = sqlx::query!("SELECT id FROM farms")
        .fetch_all(&app.db_pool)
        .await
        .expect("Failed to fetch saved farms.");

    assert_eq!(saved.len(), 1);
}

#[tokio::test]
async fn create_farm_creates_redis_key_with_response() {
    let app = spawn_app(IdempotencyEngine::Redis).await;
    seed_test_taxonomy(&app.db_pool).await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    let user = TestUser::generate_user();
    log_in_test_user(&app, &user).await;

    let body = farm_to_json(&farm, idempotency_key);

    let response = app.post_farm(&body).await;

    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let idempotency_key = IdempotencyKey::try_from(format!(
        "{}:{}:{}",
        app.configuration.idempotency.redis_key_prefix, user.id, idempotency_key
    ))
    .expect("Failed to parse idempotency key");
    let mut redis_connection = app
        .redis_pool
        .get()
        .await
        .expect("Failed to get redis connection");

    let key_exists =
        redis_exists_with_retry(&mut redis_connection, idempotency_key.as_ref(), 10, 100)
            .await
            .expect("Failed to check if key exists");
    assert!(key_exists);

    let bytes: Vec<u8> = AsyncCommands::get(&mut redis_connection, idempotency_key.as_ref())
        .await
        .expect("Failed to retrieve idempotency saved response");

    let data: IdempotencyData =
        rmp_serde::from_slice(&bytes).expect("Failed to deserialize idempotency");

    assert_eq!(response.status().as_u16(), data.response_status_code);

    let response_headers = {
        let mut h = Vec::with_capacity(response.headers().len());
        for (name, value) in response.headers().iter() {
            let name = name.as_str().to_owned();
            let value = value.as_bytes().to_owned();
            h.push(HeaderPair { name, value });
        }
        h
    };

    assert!(
        data.response_headers
            .iter()
            .all(|h| response_headers.contains(h))
    );

    assert_eq!(data.response_body, response.bytes().await.unwrap().to_vec());
}

#[tokio::test]
async fn idempotency_worker_will_not_delete_non_expired_keys() {
    let app = spawn_app(IdempotencyEngine::Postgres).await;
    let user = TestUser::generate_user();
    let now = Utc::now();
    user.store(&app.db_pool).await;

    let rows_to_create: u64 = 2;
    for _ in 0..rows_to_create {
        app.create_idempotency_row(
            user.id,
            Uuid::new_v4().to_string(),
            now.add(Duration::from_hours(1)),
        )
        .await;
    }

    let outcome = app.run_idempotency_cleanup_worker().await.unwrap();

    let idempotency_rows = app.get_idempotency_rows().await;

    assert_eq!(idempotency_rows, rows_to_create);
    assert_eq!(ExpiryOutcome::NothingToDelete, outcome);
}

#[tokio::test]
async fn idempotency_worker_will_only_delete_expired_keys() {
    let app = spawn_app(IdempotencyEngine::Postgres).await;
    let user = TestUser::generate_user();
    let now = Utc::now();
    user.store(&app.db_pool).await;

    let expired_rows_to_create: u64 = 2;
    for _ in 0..expired_rows_to_create {
        app.create_idempotency_row(
            user.id,
            Uuid::new_v4().to_string(),
            now.sub(Duration::from_hours(1)),
        )
        .await;
    }

    let non_expired_rows_to_create: u64 = 3;
    for _ in 0..non_expired_rows_to_create {
        app.create_idempotency_row(
            user.id,
            Uuid::new_v4().to_string(),
            now.add(Duration::from_hours(1)),
        )
        .await;
    }

    let outcome = app.run_idempotency_cleanup_worker().await.unwrap();

    let idempotency_rows = app.get_idempotency_rows().await;

    assert_eq!(idempotency_rows, non_expired_rows_to_create);
    assert_eq!(ExpiryOutcome::RowsDeleted(expired_rows_to_create), outcome);
}
