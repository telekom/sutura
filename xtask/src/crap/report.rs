//! Reading the CRAP policy, and reading the CRAP report.
//!
//! The PURE half of the gate: text and JSON in, verdict material out. Separate from `crap.rs`
//! because that file drives PROCESSES - two cargo subcommands, an environment to sanitise, a
//! toolchain to pin - while this one touches nothing but its arguments. The split is also what
//! makes the gate testable with neither tool installed, which is the property that stops it from
//! being a gate nobody has seen fail.
//!
//! Every refusal here closes a way the gate could pass while having judged nothing. Not
//! defensiveness for its own sake: this repo has already had a gate that "listed files via an
//! absent tool and got an empty list" and reported success.

use std::collections::BTreeMap;

/// Where the policy lives. Read by `cargo crap` itself, so a developer running the tool by hand
/// gets the same thresholds CI applies.
pub(crate) const POLICY_FILE: &str = ".cargo-crap.toml";

/// The parsed policy. Only the fields a gate has an opinion about.
#[derive(Debug, PartialEq)]
pub(crate) struct Policy {
    /// The score a function may not exceed.
    pub(crate) threshold: f64,
    /// Whether exceeding it is a failure. Anything but `true` makes the file a report.
    pub(crate) fail_above: bool,
    /// The regression tolerance, when the file states one.
    ///
    /// `Option` and not a defaulted `f64`, because the DEFAULT belongs to the module that
    /// compares - `crap::delta::DEFAULT_EPSILON`, which is deliberately cargo-crap's own default
    /// so that running the tool by hand agrees with the gate. A default baked in here would
    /// silently become a second answer to the same question.
    pub(crate) epsilon: Option<f64>,
    /// Allowlist entries, in file order.
    pub(crate) allow: Vec<AllowEntry>,
}

/// One allowlist entry and whether a comment justifies it.
#[derive(Debug, PartialEq)]
pub(crate) struct AllowEntry {
    /// The glob, without quotes.
    pub(crate) pattern: String,
    /// 1-based line number in the policy file, for the message.
    pub(crate) line: usize,
    /// Whether a `#` comment sits on this line or immediately above it.
    pub(crate) annotated: bool,
}

// ------------------------------------------------------------- the policy parser ---

/// Read the policy out of the TOML text.
///
/// TEXT SCANNING, not a TOML parser, and for the same reason `check-pins` and `check-workflows`
/// scan text: the annotation rule below is about COMMENTS, which every TOML parser discards. A
/// real parser could tell us the patterns and could never tell us whether anybody said why.
/// xtask also carries no TOML dependency, and adding one to read three keys would be a
/// dependency the `unused-deps` gate is right to ask about.
pub(crate) fn parse_policy(text: &str) -> Result<Policy, String> {
    let mut threshold = None;
    let mut fail_above = None;
    let mut epsilon = None;
    let mut allow = Vec::new();
    let mut in_allow = false;
    let mut previous_was_comment = false;

    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let is_comment = line.starts_with('#');

        if in_allow {
            if line.starts_with(']') {
                in_allow = false;
            } else if let Some(pattern) = quoted(line) {
                // A trailing comment on the entry's own line counts, and so does a comment
                // block immediately above it. Both are how a person actually writes a reason.
                let annotated = previous_was_comment || line.contains('#');
                allow.push(AllowEntry {
                    pattern,
                    line: index.saturating_add(1),
                    annotated,
                });
            }
        } else if let Some(value) = value_of(line, "threshold") {
            threshold = value.parse::<f64>().ok();
        } else if let Some(value) = value_of(line, "fail-above") {
            fail_above = Some(value == "true");
        } else if let Some(value) = value_of(line, "epsilon") {
            epsilon = value.parse::<f64>().ok();
        } else if line.starts_with("allow") && line.contains('[') {
            // `allow = []` on one line is an empty list, not the start of a block.
            in_allow = !line.contains(']');
        }

        if is_comment {
            previous_was_comment = true;
        } else if !line.is_empty() {
            previous_was_comment = false;
        }
    }

    let threshold = threshold.ok_or_else(|| format!("{POLICY_FILE} declares no numeric `threshold`"))?;
    if !threshold.is_finite() || threshold <= 0.0 {
        return Err(format!(
            "{POLICY_FILE} declares a threshold of {threshold}, which is not a line"
        ));
    }
    if fail_above != Some(true) {
        return Err(format!(
            "{POLICY_FILE} does not set `fail-above = true`, so the tool would print a report and exit 0"
        ));
    }
    Ok(Policy {
        threshold,
        fail_above: true,
        epsilon,
        allow,
    })
}

/// `key = value`, with the value trimmed of quotes, of a trailing comment, and of a Nix
/// statement terminator.
///
/// The `;` is not incidental. This reads TWO languages: `.cargo-crap.toml`, where a line ends
/// after the value, and `nix/crap.nix`, where `crapVersion = "0.4.3";` does not. Trimming quotes
/// without trimming the semicolon first yielded `0.4.3";` and the version assertion reported the
/// pin as absent from a page that named it - a gate failing on its own parser, which is the
/// failure this repo asks about before believing any verdict.
///
/// `pub(crate)` because the driver half reads the Nix pin and `rust-toolchain.toml` with it. One
/// reader for both languages, rather than a second nearly-identical one that would only be
/// nearly right.
pub(crate) fn value_of(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.trim_start().strip_prefix('=')?.trim();
    let rest = rest.split('#').next().unwrap_or(rest).trim();
    Some(String::from(rest.trim_end_matches(';').trim().trim_matches('"')))
}

/// The first double-quoted run in the line, if any.
fn quoted(line: &str) -> Option<String> {
    let (_, after) = line.split_once('"')?;
    let (inner, _) = after.split_once('"')?;
    Some(String::from(inner))
}

// ------------------------------------------------------------- the report reader ---

/// One scored function, as `cargo crap --format json` reports it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Entry {
    /// The workspace member it belongs to. Populated only in `--workspace` or `-p` mode.
    pub(crate) krate: String,
    /// Absolute path, as the tool reports it.
    pub(crate) file: String,
    /// Function path, for example `Grain::as_str`.
    pub(crate) function: String,
    /// Line the function starts on.
    pub(crate) line: u64,
    /// Cyclomatic complexity.
    pub(crate) cyclomatic: f64,
    /// Line coverage, as a percentage.
    pub(crate) coverage: f64,
    /// The CRAP score.
    pub(crate) crap: f64,
}

/// What the report says, once it has been checked for being a report at all.
#[derive(Debug, PartialEq)]
pub(crate) enum Report {
    /// Nothing was analysed, or a scoped package contributed nothing. FAIL, loudly.
    Empty(String),
    /// A usable report.
    Scored(Vec<Entry>),
}

/// Parse the JSON report and refuse one that proves nothing.
///
/// THE THREE REFUSALS, each closing a way this gate could pass while checking nothing:
///
///   * unparseable, or no `entries` array - the tool changed shape, or wrote nothing
///   * `entries` empty - the walker matched no source at all
///   * a scoped package with no entries - that crate was renamed, moved, or dropped out of the
///     analysis while the other crates kept the report looking healthy
///
/// The third is the subtle one and it is why `expected` is a parameter rather than being derived
/// from the report itself. Deriving it would make the check tautological.
pub(crate) fn read_report(json: &str, expected: &[&str]) -> Report {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(error) => return Report::Empty(format!("the report is not valid JSON: {error}")),
    };
    let Some(array) = parsed.get("entries").and_then(|e| e.as_array()) else {
        return Report::Empty(String::from("the report has no `entries` array"));
    };
    if array.is_empty() {
        return Report::Empty(String::from(
            "the report analysed no functions - the source walk found nothing, which is not a clean tree",
        ));
    }

    let entries: Vec<Entry> = array
        .iter()
        .map(|value| Entry {
            krate: string_at(value, "crate"),
            file: string_at(value, "file"),
            function: string_at(value, "function"),
            line: value.get("line").and_then(serde_json::Value::as_u64).unwrap_or(0),
            cyclomatic: number_at(value, "cyclomatic"),
            coverage: number_at(value, "coverage"),
            crap: number_at(value, "crap"),
        })
        .collect();

    let mut per_crate: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in &entries {
        *per_crate.entry(entry.krate.as_str()).or_insert(0) += 1;
    }
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|name| !per_crate.contains_key(name))
        .collect();
    if !missing.is_empty() {
        return Report::Empty(format!(
            "no scored function came from: {}. A scoped package contributing nothing means the \
             analysis missed it, not that it is clean",
            missing.join(", ")
        ));
    }

    Report::Scored(entries)
}

fn string_at(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map_or_else(String::new, String::from)
}

fn number_at(value: &serde_json::Value, key: &str) -> f64 {
    value.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
}

/// Every entry over the line, worst first.
///
/// The allowlist is NOT applied here: `cargo crap` already dropped allowed functions before
/// writing the report, so applying the patterns a second time would be a second implementation
/// of the same glob semantics - and the two would disagree eventually. What this module owns is
/// the DISCIPLINE the allowlist is written with, which the tool has no opinion about.
pub(crate) fn offenders(entries: &[Entry], threshold: f64) -> Vec<&Entry> {
    let mut over: Vec<&Entry> = entries.iter().filter(|e| e.crap > threshold).collect();
    over.sort_by(|a, b| b.crap.total_cmp(&a.crap));
    over
}

#[cfg(test)]
mod tests {
    use super::{AllowEntry, Entry, POLICY_FILE, Report, offenders, parse_policy, read_report};

    fn entry(krate: &str, function: &str, crap: f64) -> Entry {
        Entry {
            krate: String::from(krate),
            file: String::from("/abs/crates/sutura-domain/src/model.rs"),
            function: String::from(function),
            line: 1,
            cyclomatic: 6.0,
            coverage: 0.0,
            crap,
        }
    }

    fn committed_policy() -> super::Policy {
        let root = crate::repo::root().expect("repo root");
        let text = std::fs::read_to_string(root.join(POLICY_FILE)).expect("the policy file exists");
        parse_policy(&text).expect("the committed policy must parse")
    }

    #[test]
    fn the_committed_policy_parses_and_is_a_gate() {
        // The REAL file, not a fixture: a policy that stopped being a gate would otherwise be
        // caught only by the expensive task, which does not run in the Nix sandbox.
        let policy = committed_policy();
        assert!(policy.fail_above, "the policy must fail, not report");
        assert!(policy.threshold > 0.0);
    }

    #[test]
    fn every_committed_allowlist_entry_is_annotated() {
        for entry in &committed_policy().allow {
            assert!(entry.annotated, "{} has no reason beside it", entry.pattern);
        }
    }

    #[test]
    fn a_policy_that_only_reports_is_rejected() {
        // `fail-above` absent or false makes `cargo crap` print a table and exit 0. That is the
        // shape a gate quietly turns into, so it is refused by name.
        let missing = parse_policy("threshold = 30.0\n").expect_err("must be rejected");
        assert!(missing.contains("fail-above"), "{missing}");
        let disabled = parse_policy("threshold = 30.0\nfail-above = false\n").expect_err("must be rejected");
        assert!(disabled.contains("fail-above"), "{disabled}");
    }

    #[test]
    fn a_missing_or_absurd_threshold_is_rejected() {
        let absent = parse_policy("fail-above = true\n").expect_err("must be rejected");
        assert!(absent.contains("threshold"), "{absent}");
        let zero = parse_policy("threshold = 0\nfail-above = true\n").expect_err("must be rejected");
        assert!(zero.contains("not a line"), "{zero}");
    }

    #[test]
    fn an_annotation_may_be_above_the_entry_or_beside_it() {
        let policy = parse_policy(concat!(
            "threshold = 30.0\n",
            "fail-above = true\n",
            "allow = [\n",
            "  # generated match table; complexity is the arm count\n",
            "  \"Generated::*\",\n",
            "  \"src/other/**\", # a vendored port, upstream's shape\n",
            "  \"Naked::*\",\n",
            "]\n",
        ))
        .expect("parses");
        let found: Vec<(&str, bool)> = policy
            .allow
            .iter()
            .map(|e: &AllowEntry| (e.pattern.as_str(), e.annotated))
            .collect();
        assert_eq!(
            found,
            vec![("Generated::*", true), ("src/other/**", true), ("Naked::*", false)],
            "a comment above and a comment beside both count; nothing counts for the third"
        );
    }

    #[test]
    fn an_empty_allow_list_on_one_line_is_not_a_block() {
        // `allow = []` followed by more keys would otherwise swallow the rest of the file.
        let policy = parse_policy("threshold = 30.0\nallow = []\nfail-above = true\n").expect("parses");
        assert!(policy.allow.is_empty());
        assert!(policy.fail_above);
    }

    #[test]
    fn a_value_is_read_out_of_toml_and_out_of_nix() {
        use super::value_of;
        // TOML: no terminator.
        assert_eq!(value_of("threshold = 30.0", "threshold").as_deref(), Some("30.0"));
        assert_eq!(
            value_of("missing = \"pessimistic\"", "missing").as_deref(),
            Some("pessimistic")
        );
        // Nix: a `;` after the closing quote. Not trimming it produced `0.4.3";` and made the
        // version assertion fail on a page that did name the version.
        assert_eq!(value_of("crapVersion = \"0.4.3\";", "crapVersion").as_deref(), Some("0.4.3"));
        // A trailing comment is not part of the value.
        assert_eq!(value_of("threshold = 30.0  # the line", "threshold").as_deref(), Some("30.0"));
        // A different key is not a match, and neither is a bare mention.
        assert_eq!(value_of("other = 1", "threshold"), None);
        assert_eq!(value_of("threshold matters", "threshold"), None);
    }

    #[test]
    fn an_empty_report_is_refused_rather_than_read_as_clean() {
        // THE SCAR: a gate that listed nothing and said ok. Every one of these must fail.
        let cases = [
            ("not json at all", "not valid JSON"),
            ("{}", "no `entries`"),
            (r#"{"entries":[]}"#, "analysed no functions"),
        ];
        for (json, expected) in cases {
            match read_report(json, &["sutura-domain"]) {
                Report::Empty(reason) => {
                    assert!(reason.contains(expected), "{reason} lacks {expected}");
                }
                Report::Scored(_) => panic!("{json} must not read as a usable report"),
            }
        }
    }

    #[test]
    fn a_scoped_package_that_contributed_nothing_is_a_failure() {
        // The subtle hole: a renamed or moved crate drops out of the analysis while the other
        // crates keep the report looking healthy. Deriving the expected set from the report
        // itself would make this check tautological, which is why it is a parameter.
        let json = r#"{"entries":[{"crate":"sutura-domain","file":"a.rs","function":"f","line":1,
            "cyclomatic":1.0,"coverage":100.0,"crap":1.0}]}"#;
        match read_report(json, &["sutura-domain", "sutura-semantic"]) {
            Report::Empty(reason) => assert!(reason.contains("sutura-semantic"), "{reason}"),
            Report::Scored(_) => panic!("a missing scoped package must fail"),
        }
        // The same report is fine when only the package it holds was asked for.
        assert!(matches!(read_report(json, &["sutura-domain"]), Report::Scored(e) if e.len() == 1));
    }

    #[test]
    fn the_threshold_is_exclusive_and_offenders_come_worst_first() {
        let entries = vec![
            entry("sutura-domain", "at_the_line", 30.0),
            entry("sutura-domain", "over", 42.0),
            entry("sutura-domain", "well_over", 210.0),
            entry("sutura-domain", "under", 1.0),
        ];
        let over = offenders(&entries, 30.0);
        let names: Vec<&str> = over.iter().map(|e| e.function.as_str()).collect();
        // Exactly at the threshold is not over it - that is what `fail-above` means, and a gate
        // that disagreed with its own tool would be worse than either rule alone.
        assert_eq!(names, vec!["well_over", "over"]);
    }

    #[test]
    fn a_clean_report_has_no_offenders() {
        let entries = vec![entry("sutura-domain", "fine", 1.0)];
        assert!(offenders(&entries, 30.0).is_empty());
    }

    #[test]
    fn the_regression_tolerance_is_read_when_stated_and_absent_when_not() {
        // ABSENT and not defaulted here on purpose: the default belongs to the module that
        // compares, where it is deliberately cargo-crap's own, so the tool and the gate agree
        // about which functions moved. A number invented in this file would be a second answer.
        let stated = parse_policy("threshold = 30.0\nfail-above = true\nepsilon = 0.5\n").expect("parses");
        assert_eq!(stated.epsilon, Some(0.5));
        let silent = parse_policy("threshold = 30.0\nfail-above = true\n").expect("parses");
        assert_eq!(silent.epsilon, None);
        // The committed file does not state one, which is what keeps the tool and the gate in
        // step without a synchroniser.
        assert_eq!(committed_policy().epsilon, None);
    }
}
