use std::collections::BTreeMap;

use super::ServerMatch;

fn parsed(pairs: &[(&str, &str)]) -> ServerMatch {
    let raw: BTreeMap<String, String> = pairs
        .iter()
        .map(|&(name, value)| (String::from(name), String::from(value)))
        .collect();
    ServerMatch::parse(&raw).expect("a type and a field are a match")
}

/// A server's fields as a lookup, the way a reader renders a contract server.
fn rendered<'a>(fields: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    |name| {
        fields
            .iter()
            .find(|&&(field, _)| field == name)
            .map(|&(_, value)| String::from(value))
    }
}

#[test]
fn a_match_reads_its_listed_fields_and_ignores_the_rest() {
    let postgres = parsed(&[("type", "postgres"), ("host", "db.example.com"), ("port", "5432")]);
    let server = [
        ("host", "db.example.com"),
        ("port", "5432"),
        ("database", "shop"),
        ("schema", "sales"),
    ];
    assert!(postgres.matches("postgres", rendered(&server)));
    assert!(
        !postgres.matches("postgresql", rendered(&server)),
        "the type is compared exactly"
    );
    let upper = [("host", "DB.example.com"), ("port", "5432")];
    assert!(!postgres.matches("postgres", rendered(&upper)), "a host is compared exactly");
    let portless = [("host", "db.example.com")];
    assert!(
        !postgres.matches("postgres", rendered(&portless)),
        "a listed field the server lacks"
    );
}
