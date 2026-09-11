//! The parser of record for `image-digests.txt`, and the one reader every consumer reaches.
//!
//! `release.yml`'s `Push the images` step writes a three-line `#` comment header followed by `leaf`
//! and `list` records, each `kind name repository@sha256:<64 hex>` with NO tag between the
//! repository and the digest. Six readers grew up around that file - five shell `grep`/`awk` sites
//! and one Rust one - and nothing related them, so a reader that iterated every line refused the
//! header and cost a release (`telekom/sutura#598`). This module owns the whole grammar so that
//! cannot happen again: comment and blank-line skipping, exactly three whitespace-separated fields,
//! a closed `leaf`/`list` kind, and a reference whose digest is spelled and untagged. Every field is
//! split once, here.
//!
//! # One task, a reader and a guard
//!
//! Called with `<file> <kind>` from the release path it prints the records the shell signs and
//! publishes. Called with NO arguments by the `hygiene` sweep it refuses a consumer that re-splits
//! the grammar itself, which is [`super::readers`]. One task rather than two because
//! `xtask/src/main.rs` stands against the 1000-line cap `max-lines` cannot exempt, and because the
//! grammar and the rule that it has one owner are one subject.
//!
//! # The limit, next to the claim
//!
//! The guard holds DELEGATION, not correctness: it cannot prove a shell reader parses a record
//! correctly, only that no consumer re-implements the split. Grammar correctness is this module's
//! own tests, which is the point of having one reader. See [`super::readers`] for the surfaces the
//! guard reaches and the class it does not.

use std::fmt;

use crate::Verdict;

/// The two record kinds the release writes. Closed on purpose: a third spelling is a refusal, not a
/// fifth reader's coin flip about which of two spellings is the real grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Leaf,
    List,
}

impl Kind {
    /// The kind a field names, or `None` for a spelling the grammar does not carry.
    fn parse(word: &str) -> Option<Self> {
        match word {
            "leaf" => Some(Self::Leaf),
            "list" => Some(Self::List),
            _ => None,
        }
    }
}

/// One parsed record: its kind, its second field, and its reference split into the repository and
/// the 64-hex digest. The split is here rather than at each call site, which is what stops the
/// sibling action's `${repo%:*}` derivation and this module's from drifting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) repository: String,
    pub(crate) digest: String,
}

impl Record {
    /// The reference the producer writes and `cosign` signs: the repository and the digest, with no
    /// tag between them.
    pub(crate) fn reference(&self) -> String {
        format!("{}@sha256:{}", self.repository, self.digest)
    }
}

/// Why a line is not a record.
///
/// The LINE NUMBER, never the line: a typed field a machine reads and a sentence a human sees, with
/// nothing from a release record echoed into a log. The variant and its fields are the contract; the
/// `Display` wording may be reworded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseError {
    /// Not exactly three whitespace-separated fields.
    Fields { line: usize },
    /// The record kind is neither `leaf` nor `list`.
    UnknownKind { line: usize },
    /// The reference carries no `@sha256:<64 hex>` digest.
    MissingDigest { line: usize },
    /// The repository part carries a tag; the release writes none.
    Tagged { line: usize },
    /// The digest is not 64 hexadecimal digits.
    BadDigest { line: usize },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, what) = match *self {
            Self::Fields { line } => (line, "a record must have exactly three fields"),
            Self::UnknownKind { line } => (line, "a record kind must be `leaf` or `list`"),
            Self::MissingDigest { line } => (line, "a reference must carry an `@sha256:` digest"),
            Self::Tagged { line } => (line, "a repository reference must be untagged"),
            Self::BadDigest { line } => (line, "a digest must be 64 hexadecimal digits"),
        };
        write!(f, "line {line}: {what}")
    }
}

/// A parsed reference: the untagged repository and the 64-hex digest.
type Reference = (String, String);

/// Parse every record in `text`.
///
/// The input is a release job output, and it is read as what it is: a `#` comment line and a blank
/// line are skipped, and every other line must be exactly one record. Nothing is silently dropped -
/// the number of `Record`s returned is the number of accepted, non-comment, non-blank lines, and a
/// single malformed line is a refusal rather than a shorter list.
pub(crate) fn parse(text: &str) -> Result<Vec<Record>, ParseError> {
    let mut records = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut fields = trimmed.split_whitespace();
        let (Some(kind), Some(name), Some(reference)) = (fields.next(), fields.next(), fields.next()) else {
            return Err(ParseError::Fields { line });
        };
        if fields.next().is_some() {
            return Err(ParseError::Fields { line });
        }
        let Some(kind) = Kind::parse(kind) else {
            return Err(ParseError::UnknownKind { line });
        };
        let (repository, digest) = split_reference(reference, line)?;
        records.push(Record {
            kind,
            name: String::from(name),
            repository,
            digest,
        });
    }
    Ok(records)
}

/// Split `<repository>@sha256:<64 hex>` into its two halves, refusing a tagged repository.
///
/// A `:` AFTER the last `/` is a tag and is refused: the release writes none, and treating one as
/// optional silently accepts a reference that names a tag rather than a digest. A `:` BEFORE the
/// first `/` is a registry port and stays part of the repository name.
fn split_reference(reference: &str, line: usize) -> Result<Reference, ParseError> {
    let Some((repository, digest)) = reference.rsplit_once("@sha256:") else {
        return Err(ParseError::MissingDigest { line });
    };
    if repository.is_empty() {
        return Err(ParseError::MissingDigest { line });
    }
    let after_last_slash = repository.rsplit_once('/').map_or(repository, |(_, tail)| tail);
    if after_last_slash.contains(':') {
        return Err(ParseError::Tagged { line });
    }
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ParseError::BadDigest { line });
    }
    Ok((String::from(repository), String::from(digest)))
}

/// The registered entry point.
///
/// No arguments is the `hygiene` sweep: [`super::readers::enforce`]. Arguments are the release
/// path's read: `<file> leaf|list|subject [name]`.
pub(crate) fn run(args: &[String]) -> Verdict {
    if args.is_empty() {
        return super::readers::enforce();
    }
    match command(args) {
        Ok(()) => Verdict::Pass,
        Err(cause) => {
            eprintln!("xtask image-digests: {cause}");
            Verdict::Fail
        }
    }
}

/// `image-digests <file> leaf|list|subject <name>` - print records from the parser of record.
///
/// `leaf` and `list` print one `name reference` per matching record, in file order, which is the
/// order the producer wrote and the order the signing loop signs. `subject` prints the
/// `repository sha256:<hex>` pair the provenance step records as `subject-name` and
/// `subject-digest`, and refuses a name that is absent or duplicated rather than printing the first.
fn command(args: &[String]) -> Result<(), String> {
    const USAGE: &str = "usage: image-digests <file> leaf|list|subject <name>";
    let Some((path, rest)) = args.split_first() else {
        return Err(String::from(USAGE));
    };
    let Some((mode, rest)) = rest.split_first() else {
        return Err(String::from(USAGE));
    };
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    if text.trim().is_empty() {
        return Err(format!("empty image digests: {path}"));
    }
    let records = parse(&text).map_err(|error| format!("{path}: {error}"))?;
    match mode.as_str() {
        "leaf" => {
            print_records(&records, Kind::Leaf);
            Ok(())
        }
        "list" => {
            print_records(&records, Kind::List);
            Ok(())
        }
        "subject" => {
            let Some(name) = rest.first() else {
                return Err(String::from(USAGE));
            };
            print_subject(&records, name)
        }
        _ => Err(String::from(USAGE)),
    }
}

/// One `name reference` line per record of `kind`, in file order.
fn print_records(records: &[Record], kind: Kind) {
    for record in records.iter().filter(|record| record.kind == kind) {
        println!("{} {}", record.name, record.reference());
    }
}

/// The unique `list` record named `name`, as `repository sha256:<hex>`.
fn print_subject(records: &[Record], name: &str) -> Result<(), String> {
    let mut matching = records
        .iter()
        .filter(|record| record.kind == Kind::List && record.name == name);
    let Some(record) = matching.next() else {
        return Err(format!("no list record named `{name}`"));
    };
    if matching.next().is_some() {
        return Err(format!("more than one list record named `{name}`"));
    }
    println!("{} sha256:{}", record.repository, record.digest);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::{Kind, ParseError, parse};

    /// The header verbatim from the producer, which is what the reader that cost a release refused.
    const HEADER: &str = "# kind name reference@digest\n\
                          # list = multi-arch manifest list; pin this unless you want one architecture\n\
                          # leaf = single-arch image, named by its binary key and rust target triple\n";

    fn hex(n: u128) -> String {
        format!("{n:064x}")
    }

    #[test]
    fn the_header_and_blank_lines_are_skipped_without_dropping_a_record() {
        let mut text = String::from(HEADER);
        writeln!(text, "leaf native registry.example.com/test/runtime@sha256:{}", hex(1)).expect("write to a String");
        text.push('\n');
        writeln!(text, "list glibc registry.example.com/test/runtime@sha256:{}", hex(2)).expect("write to a String");
        let records = parse(&text).expect("the released shape parses");
        assert_eq!(records.len(), 2, "a skipped line hides no record");
        assert_eq!(records.first().map(|record| record.kind), Some(Kind::Leaf));
        assert_eq!(records.get(1).map(|record| record.kind), Some(Kind::List));
        assert_eq!(records.get(1).map(|record| record.name.as_str()), Some("glibc"));
    }

    #[test]
    fn a_reference_is_untagged_and_a_tag_is_forbidden() {
        let tagged = format!("list glibc registry.example.com/test/runtime:LATEST@sha256:{}\n", hex(3));
        assert_eq!(parse(&tagged), Err(ParseError::Tagged { line: 1 }));
    }

    #[test]
    fn a_registry_port_is_part_of_the_repository_rather_than_a_tag() {
        let ported = format!("list glibc registry.example.com:5000/test/runtime@sha256:{}\n", hex(4));
        let records = parse(&ported).expect("a port before the first slash is not a tag");
        assert_eq!(
            records.first().map(|record| record.repository.as_str()),
            Some("registry.example.com:5000/test/runtime")
        );
    }

    #[test]
    fn field_counts_and_kinds_are_closed() {
        assert_eq!(parse("leaf onlytwo\n"), Err(ParseError::Fields { line: 1 }));
        assert_eq!(parse("leaf a b c d\n"), Err(ParseError::Fields { line: 1 }));
        assert_eq!(parse("# comment only\n"), Ok(vec![]), "a header alone is no record");
        assert_eq!(
            parse(&format!("blob a registry.example.com/test/runtime@sha256:{}\n", hex(5))),
            Err(ParseError::UnknownKind { line: 1 })
        );
    }

    #[test]
    fn a_reference_needs_a_spelled_64_hex_digest() {
        assert_eq!(
            parse("leaf a registry.example.com/test/runtime\n"),
            Err(ParseError::MissingDigest { line: 1 })
        );
        assert_eq!(
            parse("leaf a registry.example.com/test/runtime@sha256:nothex\n"),
            Err(ParseError::BadDigest { line: 1 })
        );
        assert_eq!(
            parse(&format!(
                "leaf a registry.example.com/test/runtime@sha256:{}\n",
                "a".repeat(63)
            )),
            Err(ParseError::BadDigest { line: 1 })
        );
    }

    #[test]
    fn a_malformed_later_line_refuses_the_whole_file_rather_than_shortening_it() {
        let mut text = String::from(HEADER);
        writeln!(text, "leaf one registry.example.com/test/runtime@sha256:{}", hex(6)).expect("write to a String");
        text.push_str("list\n");
        assert_eq!(parse(&text), Err(ParseError::Fields { line: 5 }));
    }
}
