#[test]
fn coordinates_must_be_finite() {
    use farms_routing::types::Coord;

    assert!(Coord { e: 2_600_000.0, n: 1_200_000.0 }.is_finite());
    assert!(!Coord { e: f64::NAN, n: 0.0}.is_finite());
    assert!(!Coord { e: 0.0, n: f64::INFINITY}.is_finite());
}