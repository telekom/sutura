//! Reject a credential before it is committed.
//!
//! WHY THIS IS HERE AND NOT `gitleaks`. gitleaks is the mature tool and would be the first
//! choice, but it is not in the conda channel this repo can reach, and its successor
//! (betterleaks, by the same authors) ships only via brew, dnf, a container or `go install`.
//! Wiring either would mean a fourth toolchain in a repo that deliberately has three, or a
//! registry that is unreachable on some networks - and a gate that cannot run is not a gate.
//!
//! WHAT THIS IS NOT. It is not a general secret scanner. It catches the realistic accident -
//! a token pasted into a config file, a private key added to the tree, a long random string
//! assigned to something called `password` - and it will miss a secret that looks like prose.
//! If a full scanner becomes reachable, run BOTH: this one is cheap and offline.
//!
//! FAIL CLOSED. Every finding fails the gate. There is no allowlist file, deliberately: an
//! allowlist is where a real secret ends up after someone is in a hurry. A false positive is
//! fixed by making the value obviously fake, which is also better test data.
//!
//! Patterns are ASSEMBLED from parts rather than written as literals, so this file does not
//! contain the strings it looks for. Otherwise the gate reports itself and needs a
//! self-exclusion, and a self-exclusion is a hole.

use std::collections::BTreeMap;
use std::process::ExitCode;

use crate::repo;

/// Minimum length for a value to be considered a candidate secret. Below this, entropy is not
/// meaningful and every hex colour is a finding.
const MIN_LEN: usize = 20;

/// Shannon entropy per character, above which a string of `MIN_LEN` looks generated rather
/// than written. Base64 of random bytes sits near 5.0-6.0; English prose near 3.5-4.0.
const ENTROPY_FLOOR: f64 = 4.2;

/// Names that make a long random value a credential rather than a hash.
const SECRET_NAMES: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "apikey",
    "api_key",
    "api-key",
    "accesskey",
    "access_key",
    "credential",
    "client_secret",
    "private_key",
    "auth",
];

/// Extensions that legitimately hold long high-entropy strings which are not secrets.
///
/// Lock files are full of content hashes; the entropy check would fire on every line. Pattern
/// matching still applies to them, because a token in a lock file is still a token.
const HASH_HEAVY: &[&str] = &["lock", "sha256", "nix"];

/// A named credential shape.
struct Rule {
    name: &'static str,
    /// Built at runtime so this file does not contain the literal.
    needle: fn() -> String,
    /// Characters that must follow the needle, and how many, for it to be a real match.
    tail: TailShape,
}

/// What has to follow a prefix for it to be a credential rather than a mention of one.
#[derive(Clone, Copy)]
enum TailShape {
    /// At least `n` characters from the base64url alphabet.
    Base64Url(usize),
    /// At least `n` uppercase letters or digits.
    UpperAlnum(usize),
    /// Nothing required: the needle alone is conclusive.
    None,
}

fn pem_header() -> String {
    // "-----BEGIN " + ... + "PRIVATE KEY", assembled so it is not a literal here.
    let mut s = String::from("-----");
    s.push_str("BEGIN");
    s.push(' ');
    s
}

fn aws_key_prefix() -> String {
    let mut s = String::from("AK");
    s.push_str("IA");
    s
}

fn github_pat_prefix() -> String {
    let mut s = String::from("ghp");
    s.push('_');
    s
}

fn github_fine_grained_prefix() -> String {
    let mut s = String::from("github");
    s.push_str("_pat_");
    s
}

fn google_key_prefix() -> String {
    String::from("AIza")
}

fn slack_prefix() -> String {
    let mut s = String::from("xox");
    s.push('b');
    s.push('-');
    s
}

fn jwt_prefix() -> String {
    // A JWT header is almost always `{"alg":` base64url-encoded, which starts like this.
    let mut s = String::from("eyJ");
    s.push_str("hbGciOi");
    s
}

const RULES: &[Rule] = &[
    Rule {
        name: "private key block",
        needle: pem_header,
        tail: TailShape::None,
    },
    Rule {
        name: "AWS access key id",
        needle: aws_key_prefix,
        tail: TailShape::UpperAlnum(16),
    },
    Rule {
        name: "GitHub personal access token",
        needle: github_pat_prefix,
        tail: TailShape::Base64Url(36),
    },
    Rule {
        name: "GitHub fine-grained token",
        needle: github_fine_grained_prefix,
        tail: TailShape::Base64Url(22),
    },
    Rule {
        name: "Google API key",
        needle: google_key_prefix,
        tail: TailShape::Base64Url(35),
    },
    Rule {
        name: "Slack bot token",
        needle: slack_prefix,
        tail: TailShape::Base64Url(24),
    },
    Rule {
        name: "JSON Web Token",
        needle: jwt_prefix,
        tail: TailShape::Base64Url(20),
    },
];

/// One file and everything found in it.
type Offender = (String, Vec<Finding>);

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Finding {
    pub(crate) line: usize,
    pub(crate) what: String,
}

/// Shannon entropy per character, in bits.
#[expect(
    clippy::float_arithmetic,
    reason = "entropy is a logarithm over a probability distribution; integers cannot express it,               and the result is only compared against a threshold"
)]
fn entropy(text: &str) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for c in text.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "counts are bounded by line length; f64 is exact well past any line we scan"
    )]
    let total = text.chars().count() as f64;
    counts
        .values()
        .map(|&n| {
            #[expect(clippy::cast_precision_loss, reason = "same bound as above")]
            let p = n as f64 / total;
            -p * p.log2()
        })
        .sum()
}

const fn is_base64url(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Does `tail` satisfy the shape a rule requires?
fn tail_matches(tail: &str, shape: TailShape) -> bool {
    match shape {
        TailShape::None => true,
        TailShape::Base64Url(n) => tail.chars().take_while(|c| is_base64url(*c)).count() >= n,
        TailShape::UpperAlnum(n) => {
            tail.chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
                .count()
                >= n
        }
    }
}

/// The value on the right of an assignment, unquoted, if the left side names a secret.
fn assigned_secret_value(line: &str) -> Option<&str> {
    let lowered = line.to_ascii_lowercase();
    let at = SECRET_NAMES
        .iter()
        .find_map(|name| whole_word(&lowered, name).map(|start| start + name.len()))?;
    let rest = line.get(at..)?;
    let sep = rest.find(['=', ':'])?;
    let value = rest.get(sep + 1..)?.trim();
    let value = value
        .trim_start_matches(['"', '\'', '`'])
        .trim_end_matches(['"', '\'', '`', ',', ';']);
    (!value.is_empty()).then_some(value)
}

/// Where `needle` appears as a whole word, if it does.
///
/// A substring match is not good enough: `auth` sits inside `oauth-flows`, which made a router
/// entry look like an assignment of a credential. A letter or digit on either side means this
/// is a different word. `_` and `-` do not, so `client_secret` and `api-key` still match.
fn whole_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = haystack.get(from..)?.find(needle) {
        let start = from + rel;
        let before = haystack.get(..start).and_then(|s| s.chars().next_back());
        let after = haystack.get(start + needle.len()..).and_then(|s| s.chars().next());
        let bounded = |c: Option<char>| c.is_none_or(|c| !c.is_ascii_alphanumeric());
        if bounded(before) && bounded(after) {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

/// Could this value be a credential at all?
///
/// A credential is one opaque token: no whitespace, and drawn from an encoding alphabet. This
/// is the constraint that removes prose. Without it, a markdown table cell like
/// `a token: validation, claims and the downstream leg` reads as an assignment of a
/// high-entropy value, and the gate cries wolf on documentation - which it did.
fn credential_shaped(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_whitespace)
        && value.chars().all(|c| c.is_ascii_alphanumeric() || "-_+/=.:~".contains(c))
}

/// A value that is obviously a placeholder rather than a credential.
///
/// Not an allowlist of secrets - a way of saying "this is fake" in the value itself, which is
/// also how test data should read.
fn looks_like_a_placeholder(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    [
        "example",
        "changeme",
        "placeholder",
        "redacted",
        "xxx",
        "your-",
        "your_",
        "<",
        "${",
        "todo",
        "dummy",
        "fake",
        "sample",
        "test",
        "hunter2",
        "secret",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

/// Inspect one file's text.
pub(crate) fn inspect(text: &str, hash_heavy: bool) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (i, line) in text.lines().enumerate() {
        for rule in RULES {
            let needle = (rule.needle)();
            if let Some(at) = line.find(&needle) {
                let tail = line.get(at + needle.len()..).unwrap_or("");
                if tail_matches(tail, rule.tail) {
                    findings.push(Finding {
                        line: i + 1,
                        what: String::from(rule.name),
                    });
                }
            }
        }

        if hash_heavy {
            continue;
        }
        if let Some(value) = assigned_secret_value(line)
            && value.len() >= MIN_LEN
            && credential_shaped(value)
            && !looks_like_a_placeholder(value)
            && entropy(value) >= ENTROPY_FLOOR
        {
            findings.push(Finding {
                line: i + 1,
                what: String::from("high-entropy value assigned to a secret-shaped name"),
            });
        }
    }

    findings
}

pub(crate) fn run(_args: &[String]) -> ExitCode {
    let Some(repo::RepoFiles { root, files }) = repo::all_files() else {
        eprintln!("xtask check-secrets: could not determine the repo root");
        return ExitCode::FAILURE;
    };

    let mut offenders: Vec<Offender> = Vec::new();
    let mut checked = 0_usize;

    for rel in files {
        // Mirrored upstream material is not ours to edit, and its prose discusses tokens.
        if rel.starts_with(".agents/skill-library/") {
            continue;
        }
        let path = root.join(&rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue; // binary, or unreadable: nothing to scan
        };
        checked += 1;

        let hash_heavy = std::path::Path::new(&rel)
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|e| HASH_HEAVY.iter().any(|h| e.eq_ignore_ascii_case(h)));

        let findings = inspect(&text, hash_heavy);
        if !findings.is_empty() {
            offenders.push((rel, findings));
        }
    }

    if offenders.is_empty() {
        println!("xtask check-secrets: ok - {checked} file(s), {} rule(s)", RULES.len());
        return ExitCode::SUCCESS;
    }

    eprintln!("xtask check-secrets: FAILED");
    for (path, findings) in &offenders {
        for f in findings {
            eprintln!("  {path}:{}: {}", f.line, f.what);
        }
    }
    eprintln!();
    eprintln!("A committed credential is compromised even after it is deleted: it stays in the");
    eprintln!("history and in every clone. Rotate it, then remove it from the change.");
    eprintln!("A false positive means the value reads as real - make the placeholder obviously");
    eprintln!("fake (`example`, `<redacted>`, `changeme`). There is no allowlist file on purpose.");
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::{Finding, MIN_LEN, assigned_secret_value, entropy, inspect, looks_like_a_placeholder};

    // Fixtures are ASSEMBLED, never written as literals, so this file does not contain a
    // string its own rules match.
    fn fake_aws_key() -> String {
        let mut s = String::from("AK");
        s.push_str("IA");
        s.push_str("QRSTUVWX0123YZ45");
        s
    }

    fn fake_pem() -> String {
        let mut s = String::from("-----");
        s.push_str("BEGIN");
        s.push_str(" RSA PRIVATE KEY-----");
        s
    }

    fn fake_ghp() -> String {
        let mut s = String::from("ghp");
        s.push('_');
        s.push_str("Ab3Cd4Ef5Gh6Ij7Kl8Mn9Op0Qr1St2Uv3Wx4Y");
        s
    }

    #[test]
    fn finds_an_aws_access_key_id() {
        let found = inspect(&fake_aws_key(), false);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().map(|f| f.line), Some(1));
    }

    #[test]
    fn finds_a_private_key_block() {
        assert!(!inspect(&fake_pem(), false).is_empty());
    }

    #[test]
    fn finds_a_github_token() {
        assert!(!inspect(&fake_ghp(), false).is_empty());
    }

    #[test]
    fn a_mention_of_a_prefix_is_not_a_finding() {
        // The prefix alone, with no credential after it, is how documentation talks about
        // these. Flagging it would make the gate unusable in exactly the files that explain
        // it.
        let mut prose = String::from("Tokens beginning ");
        prose.push_str("ghp");
        prose.push_str("_ are personal access tokens.");
        assert_eq!(inspect(&prose, false), vec![]);
    }

    #[test]
    fn finds_a_high_entropy_value_behind_a_secret_name() {
        let line = "api_key = \"Zk3Lq9Xr7Bn2Vt5Wc8Yd4Hf6Jg0Ms1Pa\"";
        let found = inspect(line, false);
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn an_obvious_placeholder_is_not_a_finding() {
        assert_eq!(inspect("password = \"changeme-please-changeme\"", false), vec![]);
        assert_eq!(inspect("token = \"<redacted-in-this-example>\"", false), vec![]);
        assert_eq!(inspect("api_key = \"your-key-goes-right-here\"", false), vec![]);
    }

    #[test]
    fn a_secret_name_inside_another_word_is_not_a_secret_name() {
        // `auth` inside `oauth-flows` made a router entry look like a credential assignment.
        assert_eq!(
            inspect(r#"        "oauth-flows": "engineering/oauth-flows/SKILL.md""#, false),
            vec![]
        );
        // But a name joined by `_` or `-` is still the name.
        assert!(super::whole_word("client_secret = x", "secret").is_some());
        assert!(super::whole_word("api-key: x", "api-key").is_some());
        assert!(super::whole_word("oauth-flows", "auth").is_none());
    }

    #[test]
    fn prose_with_a_colon_after_a_secret_name_is_not_a_finding() {
        // The false positive that mattered: a markdown table cell. Entropy alone accepted it;
        // requiring the value to be one whitespace-free token rejects it.
        let cell = "| `oauth` | Receiving a token: validation, claims, and the downstream leg |";
        assert_eq!(inspect(cell, false), vec![]);
    }

    #[test]
    fn prose_behind_a_secret_name_is_not_a_finding() {
        // Low entropy: English, not a generated value.
        assert_eq!(
            inspect("password: the user chooses this themselves at first login", false),
            vec![]
        );
    }

    #[test]
    fn a_short_value_is_not_a_finding() {
        // Below MIN_LEN entropy says nothing; a hex colour would otherwise fire.
        let line = "secret = \"abc123\"";
        assert!(line.len() < MIN_LEN + 20);
        assert_eq!(inspect(line, false), vec![]);
    }

    #[test]
    fn hash_heavy_files_skip_the_entropy_rule_but_not_the_patterns() {
        let line = "checksum = \"9f2b8c1d4e6a7b3c5d8e0f1a2b4c6d8e0f1a2b3c4d5e6f708192a3b4c5d6e7f8\"";
        assert_eq!(inspect(line, true), vec![], "a lock file is all hashes");
        // A real token in a lock file is still a token.
        assert!(!inspect(&fake_aws_key(), true).is_empty());
    }

    #[test]
    fn entropy_separates_generated_from_written() {
        assert!(entropy("the quick brown fox jumps over") < 4.2);
        assert!(entropy("Zk3Lq9Xr7Bn2Vt5Wc8Yd4Hf6Jg0Ms1Pa") > 4.2);
    }

    #[test]
    fn reads_the_value_after_a_secret_name() {
        assert_eq!(assigned_secret_value("token = \"abc\""), Some("abc"));
        assert_eq!(assigned_secret_value("  password: hunter2  "), Some("hunter2"));
        assert_eq!(assigned_secret_value("nothing here"), None);
        // A name with no separator after it is not an assignment.
        assert_eq!(assigned_secret_value("the token rotates weekly"), None);
    }

    #[test]
    fn placeholder_detection_is_case_insensitive() {
        assert!(looks_like_a_placeholder("CHANGEME"));
        assert!(looks_like_a_placeholder("Your-Token-Here"));
        assert!(!looks_like_a_placeholder("Zk3Lq9Xr7Bn2Vt5Wc8Yd"));
    }

    #[test]
    fn line_numbers_are_one_based() {
        let mut text = String::from("clean line\n");
        text.push_str(&fake_aws_key());
        assert_eq!(
            inspect(&text, false),
            vec![Finding {
                line: 2,
                what: String::from("AWS access key id")
            }]
        );
    }
}
