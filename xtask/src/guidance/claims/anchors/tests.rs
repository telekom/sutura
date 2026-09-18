use super::is_a_bare_count;

#[test]
fn a_bare_digit_run_is_a_count() {
    assert!(is_a_bare_count("422"));
    assert!(is_a_bare_count(" 6 "));
}

#[test]
fn a_bare_number_word_is_a_count_regardless_of_case() {
    assert!(is_a_bare_count("four"));
    assert!(is_a_bare_count("Six"));
    assert!(is_a_bare_count("TWENTY"));
}

#[test]
fn a_number_inside_a_longer_literal_is_not_standalone() {
    assert!(!is_a_bare_count("mcp-e2e:"));
    assert!(!is_a_bare_count("LIMIT 10001"));
    assert!(!is_a_bare_count("six variants"));
}

#[test]
fn empty_evidence_is_not_a_count() {
    assert!(!is_a_bare_count(""));
    assert!(!is_a_bare_count("   "));
}
