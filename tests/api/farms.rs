use crate::helpers::{spawn_app, TestApp};
use chrono::Utc;
use fake::{
    faker::{address::de_de::StreetName, lorem::de_de::Word, name::de_de::Name},
    Fake,
};
use farms::domain::Point;
use farms::routes::Farm;
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

fn generate_swiss_canton() -> String {
    let cantons = vec![
        "ZH", "BE", "LU", "UR", "SZ", "OW", "NW", "GL", "ZG", "FR", "SO", "BS", "BL", "SH", "AR",
        "AI", "SG", "GR", "AG", "TG", "TI", "VD", "VS", "NE", "GE", "JU",
    ];

    let mut rng = rand::rng();
    let index = rng.random_range(0..cantons.len());
    cantons[index].to_string()
}

fn generate_farm() -> Farm {
    let id = Uuid::new_v4();
    let name: String = Name().fake();
    let address: String = StreetName().fake();
    let canton = generate_swiss_canton();
    let coordinates_str = generate_swiss_coordinates();
    let coordinates = Point::parse(&coordinates_str).expect("Generated invalid coordinates");
    let categories: Vec<String> = vec![Word().fake()];
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

fn farm_to_json(farm: &Farm) -> serde_json::Value {
    serde_json::json!({
        "id": farm.id,
        "name": farm.name,
        "address": farm.address,
        "canton": farm.canton,
        "coordinates": farm.coordinates,
        "categories": farm.categories,
        "created_at": farm.created_at,
    })
}

async fn insert_farm_in_db(app: &TestApp, farm: &Farm) {
    sqlx::query!(r#" INSERT INTO farms (id, name, address, canton, coordinates, categories, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
                farm.id,
                farm.name,
                farm.address,
                farm.canton,
                farm.coordinates as Point,
                &farm.categories,
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

#[tokio::test]
async fn get_farms_returns_empty_list_when_no_farms_exist() {
    // Arrange
    let app = spawn_app().await;

    // Act
    let response = app.get_farms().await;

    // Assert
    assert_eq!(200, response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 0);
}

#[tokio::test]
async fn get_farms_returns_200_and_list_of_farms() {
    // Arrange
    let app = spawn_app().await;
    let created_farm = create_single_farm(&app).await;

    // Act
    let response = app.get_farms().await;

    println!("Response status: {}", &response.status());

    // Assert
    assert_eq!(200, response.status().as_u16());

    let farms: Vec<Farm> = response
        .json()
        .await
        .expect("Failed to parse response as JSON.");

    assert_eq!(farms.len(), 1);
    assert_eq!(farms[0].name, created_farm.name);
    assert_eq!(farms[0].canton, created_farm.canton);
}

#[tokio::test]
async fn get_farms_returns_500_when_unexpected_error_occurs() {
    // Arrange
    let app = spawn_app().await;
    // Break the DB
    break_farms_table(&app).await;

    // Act
    let response = app.get_farms().await;

    println!("Response status: {}", &response.status());
    // Assert
    assert_eq!(500, response.status().as_u16());
}

#[tokio::test]
async fn create_farm_returns_a_200_for_valid_body_data() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();

    // Act
    let body = farm_to_json(&farm);

    let response = app.post_farm(body).await;

    // Assert
    assert_eq!(200, response.status().as_u16());

    let saved = sqlx::query!(
        r#"
        SELECT id, name, address, canton, coordinates as "coordinates: Point", categories
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
    assert_eq!(
        saved.categories.into_iter().collect::<HashSet<_>>(),
        farm.categories
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>()
    );
}

#[tokio::test]
async fn create_farm_returns_a_500_when_unexpected_error_occurs() {
    // Arrange
    let app = spawn_app().await;
    let farm = generate_farm();
    // Break the DB
    break_farms_table(&app).await;

    // Act
    let body = farm_to_json(&farm);
    let response = app.post_farm(body).await;

    // Assert
    assert_eq!(500, response.status().as_u16());
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
                ]
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
                ]
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
                ]
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
                ]
            }),
            "missing field 'coordinates'",
        ),
        (
            serde_json::json!({
                "name": "Farmy",
                "address": "Bahnhofstrasse, 5401 Baden",
                "canton": "Aargau",
                "coordinates": "47.3769,8.5417",
            }),
            "missing field 'categories'",
        ),
    ];

    for (invalid_body, error_message) in test_cases {
        // Act
        let response = app.post_farm(invalid_body).await;

        // Assert
        assert_eq!(
            400,
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
                "categories": ["Dairy"]
            }),
            "invalid coordinate format",
        ),
        (
            serde_json::json!({
                "name": "Test Farm",
                "address": "Test Address",
                "canton": "ZH",
                "coordinates": "47.3769",
                "categories": ["Dairy"]
            }),
            "single number coordinate",
        ),
        (
            serde_json::json!({
                "name": "Test Farm",
                "address": "Test Address",
                "canton": "ZH",
                "coordinates": "abc,def",
                "categories": ["Dairy"]
            }),
            "non-numeric coordinates",
        ),
    ];

    for (invalid_body, error_message) in test_cases {
        // Act
        let response = app.post_farm(invalid_body).await;

        // Assert
        assert_eq!(
            400,
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
        "categories": ["Dairy"]
    });

    // Act
    let response = app.post_farm(body).await;

    // Assert
    assert_eq!(400, response.status().as_u16());
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
        "categories": ["Dairy"]
    });

    // Act
    let response = app.post_farm(body).await;

    // Assert
    assert_eq!(400, response.status().as_u16());
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
        "categories": ["Dairy"]
    });

    // Act
    let response = app.post_farm(body).await;

    // Assert
    assert_eq!(400, response.status().as_u16());
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
            "categories": ["Dairy"]
        });

        // Act
        let response = app.post_farm(body).await;

        // Assert
        assert_eq!(
            200,
            response.status().as_u16(),
            "Failed to create farm for canton {}",
            canton
        );
    }
}
