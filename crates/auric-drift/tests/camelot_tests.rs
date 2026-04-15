use auric_drift::CamelotWheel;

#[test]
fn same_key_returns_one() {
    assert_eq!(CamelotWheel::compatibility(0, 0), 1.0);
    assert_eq!(CamelotWheel::compatibility(15, 15), 1.0);
}

#[test]
fn relative_major_minor_is_high() {
    // C major (0) and A minor (21) share Camelot number 8
    let score = CamelotWheel::compatibility(0, 21);
    assert!(score >= 0.9, "relative major/minor should be >= 0.9, got {score}");
}

#[test]
fn adjacent_same_letter_is_high() {
    // C major (8B) and G major (9B) are adjacent
    let score = CamelotWheel::compatibility(0, 7);
    assert!(score >= 0.85, "adjacent keys should be >= 0.85, got {score}");
}

#[test]
fn distant_keys_are_low() {
    // C major (8B) and F# major (2B) are far apart
    let score = CamelotWheel::compatibility(0, 6);
    assert!(score <= 0.4, "distant keys should be <= 0.4, got {score}");
}

#[test]
fn out_of_range_returns_default() {
    assert_eq!(CamelotWheel::compatibility(25, 0), 0.5);
    assert_eq!(CamelotWheel::compatibility(-1, 0), 0.5);
}

#[test]
fn key_names_cover_all_24() {
    for i in 0..24 {
        let name = CamelotWheel::name(i);
        assert_ne!(name, "?", "key {i} should have a name");
    }
    assert_eq!(CamelotWheel::name(24), "?");
}
