use auric_drift::analyzer::DriftAnalyzer;
use std::path::PathBuf;

#[test]
fn rejects_nonexistent_file() {
    let analyzer = DriftAnalyzer::new();
    let result = analyzer.analyze_file(&PathBuf::from("/definitely/not/a/file.flac"));
    assert!(result.is_err());
}

#[test]
fn normalize_returns_clamped_values() {
    assert_eq!(auric_drift::analyzer::clamp_normalize(50.0, 0.0, 100.0), 0.5);
    assert_eq!(auric_drift::analyzer::clamp_normalize(-10.0, 0.0, 100.0), 0.0);
    assert_eq!(auric_drift::analyzer::clamp_normalize(200.0, 0.0, 100.0), 1.0);
}

#[test]
fn batch_analyze_empty_returns_empty() {
    let analyzer = DriftAnalyzer::new();
    let results = analyzer.analyze_batch(&[], None);
    assert!(results.is_empty());
}
