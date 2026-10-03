//! A release is refused without every adapter it ships - `github.com/telekom/sutura#1247`.
//!
//! `checks.shipped-features` fails naming any crate of its `required` list the built binary does
//! not embed, but it runs only where a release is built. This rule holds that list at PR time:
//! every adapter crate a shipped feature pulls (a `dep:sutura-exec-*` or `dep:sutura-catalog-*`
//! item of the package's `[features]`) must be in `required`, and every adapter crate `required`
//! names must be pulled by a shipped feature. So an adapter dropped from `features` while
//! `required` still names it is refused here, naming the crate, and one added without a
//! requirement is refused the same way.
//!
//! **Limits.** Line-oriented like the rest of this gate: one `[features]` entry per manifest line,
//! a record's `features` list on one line, and `required = { ... };` closed by the first `};`
//! after it. A list broken across lines reads short, which refuses a correct tree rather than
//! passing a wrong one. An adapter crate pulled only through another crate's feature is not seen.
//! That the release then embeds what `required` names is the nix check's to prove, not this one's.
//!
//! Its own file because the parent is against the unexemptable 1000-line cap.

use super::Record;

/// Whether a crate is an adapter: a data or a metadata source.
fn is_adapter(name: &str) -> bool {
    name.starts_with("sutura-exec-") || name.starts_with("sutura-catalog-")
}

/// One `[features]` entry: its name and the adapter crates it pulls as a `dep:`.
type Pulls = (String, Vec<String>);

/// Each `[features]` entry of a manifest, in order.
fn pulled(manifest: &str) -> Vec<Pulls> {
    let mut out = Vec::new();
    let mut in_features = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_features = trimmed == "[features]";
            continue;
        }
        if !in_features || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let crates = super::quoted_items(value)
            .into_iter()
            .filter_map(|item| item.strip_prefix("dep:").filter(|c| is_adapter(c)).map(String::from))
            .collect();
        out.push((String::from(name.trim()), crates));
    }
    out
}

/// The crates `required = { <bin> = [ ... ]; };` names for `bin`, in order.
fn required(nix: &str, bin: &str) -> Vec<String> {
    let Some(block) = nix.find("required = {").and_then(|at| nix.get(at..)) else {
        return Vec::new();
    };
    let block = block.find("};").and_then(|end| block.get(..end)).unwrap_or(block);
    let key = format!("{bin} = [");
    let Some(list) = block.find(&key).and_then(|at| block.get(at.saturating_add(key.len())..)) else {
        return Vec::new();
    };
    super::quoted_items(list.find(']').and_then(|end| list.get(..end)).unwrap_or(list))
}

/// Every way one record's shipped `features` and its `required` list disagree, worded.
pub(super) fn unrequired(record: &Record, nix: &str, manifest: &str) -> Vec<String> {
    let bin = &record.bin;
    let required = required(nix, bin);
    if required.is_empty() {
        return vec![format!("{bin}: read no `required = {{ {bin} = [ ... ]; }}` list")];
    }
    let pulled = pulled(manifest);
    let mut out = Vec::new();
    for (feature, crates) in pulled.iter().filter(|(feature, _)| record.features.contains(feature)) {
        for name in crates.iter().filter(|name| !required.contains(name)) {
            out.push(format!(
                "{bin}: shipped feature `{feature}` pulls `{name}`, which `required` does not name"
            ));
        }
    }
    for name in &required {
        let by: Vec<&str> = pulled
            .iter()
            .filter(|(_, crates)| crates.contains(name))
            .map(|(feature, _)| feature.as_str())
            .collect();
        if !by.is_empty()
            && !by
                .iter()
                .any(|feature| record.features.iter().any(|shipped| shipped == feature))
        {
            out.push(format!(
                "{bin}: `required` names `{name}`, but no shipped feature pulls it ({} not in `features`)",
                by.join(", ")
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Record, unrequired};

    const MANIFEST: &str = "[features]\n\
        tls = [\"sutura-http/tls\"]\n\
        bigquery = [\"dep:sutura-exec-bigquery\", \"sutura-exec-bigquery/adbc\"]\n\
        postgres = [\"dep:sutura-exec-postgres\"]\n\
        rdbms = [\"dep:sutura-catalog-rdbms\", \"dep:sutura-exec-postgres\"]\n\
        \n\
        [dependencies]\n\
        serde = { workspace = true }\n";

    fn shipping(features: &[&str]) -> Record {
        Record {
            bin: String::from("sutura"),
            package: String::from("sutura-cli"),
            probe_features: Vec::new(),
            features: features.iter().map(|feature| String::from(*feature)).collect(),
        }
    }

    fn nix(required: &str) -> String {
        format!("        required = {{ sutura = [ {required} ]; }};\n        forbidden = [ \"ring\" ];\n")
    }

    #[test]
    fn an_adapter_dropped_from_features_is_refused_naming_its_crate() {
        // #1247's case: `bigquery` leaves the shipped `features` while `required` still names its
        // crate, so a release would fail only where one is built. Refused here, by name.
        let found = unrequired(
            &shipping(&["tls", "postgres"]),
            &nix(r#""axum" "sutura-exec-bigquery" "sutura-exec-postgres""#),
            MANIFEST,
        );
        assert_eq!(
            found,
            ["sutura: `required` names `sutura-exec-bigquery`, but no shipped feature pulls it (bigquery not in `features`)"]
        );
    }

    #[test]
    fn a_shipped_adapter_required_by_nothing_is_refused_naming_its_crate() {
        // The other direction: a shipped adapter `required` does not name ships unchecked.
        let found = unrequired(
            &shipping(&["tls", "bigquery", "postgres"]),
            &nix(r#""axum" "sutura-exec-postgres""#),
            MANIFEST,
        );
        assert_eq!(
            found,
            ["sutura: shipped feature `bigquery` pulls `sutura-exec-bigquery`, which `required` does not name"]
        );
    }

    #[test]
    fn a_required_list_that_matches_the_shipped_adapters_refuses_nothing() {
        // `rdbms` is not shipped and pulls `sutura-exec-postgres` too: a crate some shipped
        // feature pulls is held, whatever else also pulls it.
        let found = unrequired(
            &shipping(&["tls", "bigquery", "postgres"]),
            &nix(r#""axum" "sutura-exec-bigquery" "sutura-exec-postgres""#),
            MANIFEST,
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_required_list_it_cannot_read_is_refused() {
        // Fail closed: no list read is not "nothing required".
        let found = unrequired(&shipping(&["bigquery"]), "        forbidden = [ \"ring\" ];\n", MANIFEST);
        assert_eq!(found, ["sutura: read no `required = { sutura = [ ... ]; }` list"]);
    }
}
