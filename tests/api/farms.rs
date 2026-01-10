use crate::helpers::{TestApp, redis_exists_with_retry, spawn_app};
use actix_web::http::StatusCode;
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use fake::{
    Fake,
    faker::{address::de_de::StreetName, name::de_de::Name as FakerName},
};
use farms::{
    domain::farm::{Address, Canton, Categories, Name, Point},
    idempotency::{HeaderPair, IdempotencyData, IdempotencyKey},
    routes::farms::Farm,
};
use rand::Rng;
use std::collections::HashSet;
use uuid::Uuid;

/// Generate a valid Swiss coordinate within Switzerland boundaries
fn generate_swiss_coordinates() -> String {
    // Generate coordinates within Switzerland boundaries
    // Latitude: 45.8 to 47.9
    // Longitude: 5.9 to 10.6
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

fn generate_farm() -> Farm {
    let id = Uuid::new_v4();
    let name = Name::parse(FakerName().fake()).expect("Generated invalid farm name");
    let address = Address::parse(StreetName().fake()).expect("Generated invalid address");
    let canton = generate_swiss_canton();
    let coordinates_str = generate_swiss_coordinates();
    let coordinates = Point::parse(&coordinates_str).expect("Generated invalid coordinates");
    let categories =
        Categories::parse(vec!["Organic".to_string(), "Vegetables".to_string()]).unwrap();
    let created_at = Utc::now();

    Farm {
        id,
        name,
        address,
        canton,
        coordinates,
        categories,
        created_at,
        updated_at: None,
    }
}

fn farm_to_json(farm: &Farm, idempotency_key: Uuid) -> serde_json::Value {
    serde_json::json!({
        "id": farm.id,
        "name": farm.name,
        "address": farm.address,
        "canton": farm.canton,
        "coordinates": farm.coordinates,
        "categories": farm.categories,
        "created_at": farm.created_at,
        "idempotency_key": idempotency_key.to_string(),
    })
}

async fn insert_farm_in_db(app: &TestApp, farm: &Farm) {
    sqlx::query!(r#" INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                farm.id,
                &farm.name as &Name,
                &farm.address as &Address,
                &farm.canton as &Canton,
                &farm.coordinates as &Point,
                &farm.categories as &Categories,
                farm.created_at,
            )
        .execute(&app.db_pool)
        .await
        .expect("Failed to execute query");
}

async fn break_farms_table(app: &TestApp) {
    sqlx::query!("ALTER TABLE farms DROP COLUMN name;",)
        .execute(&app.db_pool)
        .await
        .expect("Failed to execute query");
}

async fn create_single_farm(app: &TestApp) -> Farm {
    let farm = generate_farm();

    // Insert test data
    insert_farm_in_db(app, &farm).await;

    farm
}

// Can be optimized
// generate all farms details and then batch insert them
async fn create_n_farms(app: &TestApp, n: usize) -> Vec<Farm> {
    let mut farms = Vec::<Farm>::with_capacity(n);

    for _ in 0..n {
        let farm = create_single_farm(app).await;
        farms.push(farm);
    }

    farms
}

#[tokio::test]
async fn get_farms_returns_empty_list_when_no_farms_exist() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app.get_farms().await;

    // Assert
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 0);
}

#[tokio::test]
async fn get_farms_returns_200_and_a_single_farm() {
    // Arrange
    let app = spawn_app().await;
    let created_farm = create_single_farm(&app).await;

    // Act
    let response = app.get_farms().await;

    // Assert
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 1);
    assert_eq!(farms[0].name, created_farm.name);
    assert_eq!(farms[0].canton, created_farm.canton);
}

#[tokio::test]
async fn get_farms_returns_200_and_list_of_farms() {
    // Arrange
    let app = spawn_app().await;
    let n_farms = 10;
    let created_farms = create_n_farms(&app, n_farms).await;

    // Act
    let response = app.get_farms().await;

    // Assert
    assert_eq!(StatusCode::OK.as_u16(), response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), created_farms.len());

    for created_farm in &created_farms {
        assert!(farms.iter().any(|f| f.id == created_farm.id));
    }
}

#[tokio::test]
async fn get_farms_returns_500_when_unexpected_error_occurs() {
    // Arrange
    let app = spawn_app().await;
    // Break the DB
    break_farms_table(&app).await;

    // Act
    let response = app.get_farms().await;

    // Assert
    assert_eq!(
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn create_farm_returns_a_200_for_valid_body_data() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    // Act
    let body = farm_to_json(&farm, idempotency_key);

    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let saved = sqlx::query!(
        r#"
        SELECT id, name as "name: Name", address as "address: Address", canton as "canton: Canton", coordinates as "coordinates: Point", categories as "categories: Categories"
        FROM farms
        "#
    )
    .fetch_one(&app.db_pool)
    .await
    .expect("Failed to fetch saved subscription.");

    assert_eq!(saved.name, farm.name);
    assert_eq!(saved.address, farm.address);
    assert_eq!(saved.canton, farm.canton);
    assert_eq!(saved.coordinates, farm.coordinates);
    assert_eq!(saved.categories, farm.categories);
}

#[tokio::test]
async fn create_farm_returns_a_500_when_unexpected_error_occurs() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();
    // Break the DB
    break_farms_table(&app).await;

    // Act
    let body = farm_to_json(&farm, idempotency_key);
    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(
        StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        response.status().as_u16()
    );
}

#[tokio::test]
async fn create_farm_returns_a_400_for_invalid_body_data() {
    // Arrange
    let app = spawn_app().await;
    let test_cases = vec![
        (
            serde_json::json!({
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "Aargau",
                "coordinates": "47.3769,8.5417",
                "categories": [
                    "Organic",
                    "Fruit",
                    "Vegetables"
                ],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'name'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "canton": "Aargau",
                "coordinates": "47.3769,8.5417",
                "categories": [
                    "Organic",
                    "Fruit",
                    "Vegetables"
                ],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'address'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "coordinates": "47.3769,8.5417",
                "categories": [
                    "Organic",
                    "Fruit",
                    "Vegetables"
                ],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'canton'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "Aargau",
                "categories": [
                    "Organic",
                    "Fruit",
                    "Vegetables"
                ],
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'coordinates'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "Aargau",
                "coordinates": "47.3769,8.5417",
                "idempotency_key": Uuid::new_v4(),
            }),
            "missing field 'categories'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "Aargau",
                "coordinates": "47.3769,8.5417",
                "categories": [
                    "Organic",
                    "Fruit",
                    "Vegetables"
                ],
            }),
            "missing field 'idempotency_key'",
        ),
    ];

    for (invalid_body, error_message) in test_cases {
        // Act
        let response = app.post_farm(&invalid_body).await;

        // Assert
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
    // Arrange
    let app = spawn_app().await;

    let test_cases = vec![
        (
            serde_json::json!({
                "name": "Test Farm",
                "address": "Test Address",
                "canton": "ZH",
                "coordinates": "invalid",
                "categories": ["Dairy"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "invalid coordinate format",
        ),
        (
            serde_json::json!({
                "name": "Test Farm",
                "address": "Test Address",
                "canton": "ZH",
                "coordinates": "47.3769",
                "categories": ["Dairy"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "single number coordinate",
        ),
        (
            serde_json::json!({
                "name": "Test Farm",
                "address": "Test Address",
                "canton": "ZH",
                "coordinates": "abc,def",
                "categories": ["Dairy"],
                "idempotency_key": Uuid::new_v4(),
            }),
            "non-numeric coordinates",
        ),
    ];

    for (invalid_body, error_message) in test_cases {
        // Act
        let response = app.post_farm(&invalid_body).await;

        // Assert
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
    // Arrange
    let app = spawn_app().await;

    let body = serde_json::json!({
        "name": "Berlin Farm",
        "address": "Berlin Street 1",
        "canton": "ZH",
        "coordinates": "52.5200,13.4050",  // Berlin coordinates
        "categories": ["Dairy"],
        "idempotency_key": Uuid::new_v4(),
    });

    // Act
    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_for_invalid_latitude() {
    // Arrange
    let app = spawn_app().await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Test Address",
        "canton": "ZH",
        "coordinates": "91.0,8.5417",  // Latitude > 90
        "categories": ["Dairy"],
        "idempotency_key": Uuid::new_v4(),
    });

    // Act
    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_400_for_invalid_longitude() {
    // Arrange
    let app = spawn_app().await;

    let body = serde_json::json!({
        "name": "Test Farm",
        "address": "Test Address",
        "canton": "ZH",
        "coordinates": "47.3769,181.0",  // Longitude > 180
        "categories": ["Dairy"],
        "idempotency_key": Uuid::new_v4(),
    });

    // Act
    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(StatusCode::BAD_REQUEST.as_u16(), response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_200_for_all_valid_swiss_cantons() {
    // Arrange
    let app = spawn_app().await;

    let cantons = vec![
        "ZH", "BE", "LU", "UR", "SZ", "OW", "NW", "GL", "ZG", "FR", "SO", "BS", "BL", "SH", "AR",
        "AI", "SG", "GR", "AG", "TG", "TI", "VD", "VS", "NE", "GE", "JU",
    ];

    for canton in cantons {
        let body = serde_json::json!({
            "name": format!("{} Farm", canton),
            "address": "Test Address",
            "canton": canton,
            "coordinates": "47.3769,8.5417",
            "categories": ["Dairy"],
            "idempotency_key": Uuid::new_v4(),
        });

        // Act
        let response = app.post_farm(&body).await;

        // Assert
        assert_eq!(
            StatusCode::CREATED.as_u16(),
            response.status().as_u16(),
            "Failed to create farm for canton {}",
            canton
        );
    }
}

#[tokio::test]
async fn create_farm_called_multiple_times_sequentially_doesnt_create_duplicate_farms_in_db() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    // Act
    let body = farm_to_json(&farm, idempotency_key);

    let response1 = app.post_farm(&body).await;
    let response2 = app.post_farm(&body).await;

    // Assert
    assert_eq!(response1.status(), StatusCode::CREATED.as_u16());
    assert_eq!(response2.status(), StatusCode::CREATED.as_u16());

    let saved = sqlx::query!("SELECT id FROM farms",)
        .fetch_all(&app.db_pool)
        .await
        .expect("Failed to fetch saved farms.");

    assert_eq!(saved.len(), 1);
}

#[tokio::test]
async fn create_farm_called_multiple_times_in_parallel_doesnt_create_duplicate_farms_in_db() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    // Act
    let body = farm_to_json(&farm, idempotency_key);

    let response1 = app.post_farm(&body);
    let response2 = app.post_farm(&body);
    let (response1, response2) = tokio::join!(response1, response2);

    // Assert
    let status1 = response1.status().as_u16();
    let status2 = response2.status().as_u16();

    let allowed_statuses =
        HashSet::from([StatusCode::CREATED.as_u16(), StatusCode::CONFLICT.as_u16()]);

    assert!(allowed_statuses.contains(&status1));
    assert!(allowed_statuses.contains(&status2));

    // At least one must be CREATED
    assert!(status1 == StatusCode::CREATED.as_u16() || status2 == StatusCode::CREATED.as_u16());

    let saved = sqlx::query!("SELECT id FROM farms")
        .fetch_all(&app.db_pool)
        .await
        .expect("Failed to fetch saved farms.");

    assert_eq!(saved.len(), 1);
}

#[tokio::test]
async fn create_farm_creates_redis_key_with_response() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    let idempotency_key = Uuid::new_v4();

    // Act
    let body = farm_to_json(&farm, idempotency_key);

    let response = app.post_farm(&body).await;

    // Assert
    assert_eq!(StatusCode::CREATED.as_u16(), response.status().as_u16());

    let idempotency_key = IdempotencyKey::try_from(format!(
        "{}:{}",
        app.configuration.idempotency.redis_key_prefix,
        idempotency_key.to_string()
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
