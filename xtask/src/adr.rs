//! What a record under `docs/adr/` is named, and `new-adr`, which mints one.
//!
//! **Why a new identifier is not an ordinal: `github.com/telekom/sutura#937`.** An ordinal is a
//! resource two unpushed branches contend for, and the collision rule in `crate::guidance::pages`
//! sees only the merged tree - so it refuses the second branch in the merge queue, after both had
//! chosen. The records already numbered keep their numbers, because every citation names them. A
//! NEW record is named by the UTC second it was minted in, `YYYYMMDDHHMMSS-<slug>.md`, which sorts
//! after every ordinal and which two branches share only by minting in the same second. [`Id::of`]
//! is the one parse both the mint and the gate read, so the two cannot disagree on the shape.
//!
//! # What it does not reach
//!
//! * **The same second on two branches.** Still a collision, still refused, by the same rule in the
//!   same venue. The identifier makes a collision improbable, not impossible.
//! * **When a name was written.** Any real UTC second parses, so a hand-typed or back-dated
//!   identifier passes. What is held is that the name IS a second and is unique, not a chronology.
//! * **A slug the mint did not write.** The gate asks only that one exists; [`is_slug`]'s
//!   character rule is the mint's.

use std::fmt;
use std::io::Write as _;
use std::path::Path;

use crate::Verdict;

/// The last ordinal a record may carry. 0039 and 0040 were written by work open when #937 was
/// decided and are retained with the rest; nothing past this is minted as an ordinal again.
const LAST_ORDINAL: u16 = 40;

/// Inside the frozen range and never merged, so a record naming it now would be a new ordinal.
const NEVER_MERGED: u16 = 37;

/// Why a record's file name is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// No leading digit run, or no slug after it - nothing the collision rule could compare.
    Unreadable,
    /// A four-digit ordinal outside the frozen set.
    NewOrdinal(u16),
    /// A digit run that is neither a frozen ordinal nor a real UTC second.
    NotASecond,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Unreadable => f.write_str(
                "an ADR is named `<identifier>-<slug>.md`, and this name carries no identifier the \
                 collision rule can read - so nothing would compare it against the record that holds \
                 the same one",
            ),
            Self::NewOrdinal(ordinal) => write!(
                f,
                "{ordinal:04} is a new ordinal - the numbered records are frozen at 0001-{LAST_ORDINAL:04} \
                 ({NEVER_MERGED:04} was never merged), because an ordinal is what two unpushed branches \
                 collide on; mint the record with `just new-adr <slug>`"
            ),
            Self::NotASecond => f.write_str(
                "a new ADR is named by its UTC creation second, `YYYYMMDDHHMMSS`, and this digit run is \
                 neither that nor a frozen four-digit ordinal; mint the record with `just new-adr <slug>`",
            ),
        }
    }
}

/// A record's identifier: a frozen ordinal, or the UTC second it was minted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Id {
    Ordinal(u16),
    Minted(Minted),
}

impl Id {
    /// The identifier a record's file NAME claims - `docs/adr/` already stripped.
    ///
    /// **The digit RUN, not the text before the first `-`, and that is a measured hole.** A reader
    /// that took the text before the first `-` answered *not judged* for a bare `0037.md`, and
    /// `check-guidance` printed `ok` over it sitting beside `0037-a-real-record.md` - one ordinal,
    /// two records. So a name with no digit run or no slug is refused rather than skipped: a naming
    /// this cannot read is a way past the collision rule, not an untidiness.
    pub(crate) fn of(name: &str) -> Result<Self, Refused> {
        let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
        let digits = name.strip_suffix(rest).unwrap_or(name);
        let slugged = rest
            .strip_prefix('-')
            .and_then(|tail| tail.strip_suffix(".md"))
            .is_some_and(|slug| !slug.is_empty());
        if digits.is_empty() || !slugged {
            return Err(Refused::Unreadable);
        }
        if digits.len() == 4 {
            let ordinal = digits.parse::<u16>().unwrap_or(0);
            return if (1..=LAST_ORDINAL).contains(&ordinal) && ordinal != NEVER_MERGED {
                Ok(Self::Ordinal(ordinal))
            } else {
                Err(Refused::NewOrdinal(ordinal))
            };
        }
        Minted::parse(digits).map(Self::Minted).ok_or(Refused::NotASecond)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Ordinal(ordinal) => write!(f, "{ordinal:04}"),
            Self::Minted(minted) => minted.fmt(f),
        }
    }
}

/// A real UTC second, written `YYYYMMDDHHMMSS`. Fields in significance order, so `Ord` is time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Minted {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
}

impl Minted {
    /// Fourteen digits naming a second that exists - no 30 February, no hour 24 - or `None`.
    fn parse(digits: &str) -> Option<Self> {
        if digits.len() != 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let field = |from: usize, to: usize| digits.get(from..to)?.parse::<u64>().ok();
        let minted = Self {
            year: field(0, 4)?,
            month: field(4, 6)?,
            day: field(6, 8)?,
            hour: field(8, 10)?,
            minute: field(10, 12)?,
            second: field(12, 14)?,
        };
        let real = minted.year >= 1970
            && (1..=12).contains(&minted.month)
            && (1..=days_in(minted.year, minted.month)).contains(&minted.day)
            && minted.hour < 24
            && minted.minute < 60
            && minted.second < 60;
        real.then_some(minted)
    }

    /// The UTC second `since_epoch` seconds after 1970-01-01T00:00:00Z.
    const fn at(since_epoch: u64) -> Self {
        let (mut days, of_day) = (since_epoch.div_euclid(86_400), since_epoch.rem_euclid(86_400));
        let mut year = 1970;
        while days >= days_in_year(year) {
            days -= days_in_year(year);
            year += 1;
        }
        let mut month = 1;
        while days >= days_in(year, month) {
            days -= days_in(year, month);
            month += 1;
        }
        Self {
            year,
            month,
            day: days + 1,
            hour: of_day.div_euclid(3600),
            minute: of_day.rem_euclid(3600).div_euclid(60),
            second: of_day.rem_euclid(60),
        }
    }
}

impl fmt::Display for Minted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        } = *self;
        write!(f, "{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
    }
}

const fn is_leap(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

const fn days_in_year(year: u64) -> u64 {
    if is_leap(year) { 366 } else { 365 }
}

const fn days_in(year: u64, month: u64) -> u64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Why [`mint`] wrote nothing.
#[derive(Debug)]
enum MintRefused {
    /// Not lowercase ASCII words joined by single `-`, the way every record here is named.
    NotASlug(String),
    /// A name the gate would refuse - unreachable while [`Minted::at`] and [`Id::of`] agree, and
    /// checked so that a disagreement refuses here rather than in the merge queue.
    Name(String, Refused),
    /// `mkdocs.yml`'s `exclude_docs` block lists no `adr/` record to list this one beside.
    NoExcludedRecord,
    /// A read or write failed; the path is repo-relative. An existing record is `AlreadyExists`.
    Io(String, std::io::Error),
}

impl fmt::Display for MintRefused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotASlug(slug) => write!(f, "`{slug}` is not a slug - lowercase ASCII words joined by single `-`"),
            Self::Name(name, refused) => write!(f, "{name}: {refused}"),
            Self::NoExcludedRecord => f.write_str("mkdocs.yml: `exclude_docs` lists no `adr/` record to list this one beside"),
            Self::Io(rel, why) => write!(f, "{rel}: {why}"),
        }
    }
}

fn is_slug(slug: &str) -> bool {
    slug.split('-')
        .all(|word| !word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// `mkdocs.yml`'s text with `  adr/<name>` after the last record the `exclude_docs` block lists,
/// or `None` when that block lists none to sit beside. Every ADR is excluded from the site by a
/// literal line, and `check-docs` refuses a page in neither `nav` nor that list.
fn excluded(text: &str, name: &str) -> Option<String> {
    let entry = format!("  adr/{name}");
    let mut lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| line.starts_with("exclude_docs:"))? + 1;
    let block = lines.iter().skip(start).take_while(|line| line.starts_with("  ")).count();
    let last = lines
        .get(start..start + block)?
        .iter()
        .rposition(|line| line.starts_with("  adr/"))?;
    lines.insert(start + last + 1, &entry);
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Some(out)
}

/// Write `docs/adr/<at>-<slug>.md` under `root` and list it in `mkdocs.yml`; the repo-relative path.
///
/// The record is created exclusively BEFORE `mkdocs.yml` is touched, so a second mint of one name
/// refuses without listing anything.
fn mint(root: &Path, slug: &str, at: Minted) -> Result<String, MintRefused> {
    if !is_slug(slug) {
        return Err(MintRefused::NotASlug(slug.to_owned()));
    }
    let name = format!("{at}-{slug}.md");
    if let Err(refused) = Id::of(&name) {
        return Err(MintRefused::Name(name, refused));
    }
    let mkdocs = root.join("mkdocs.yml");
    let text = std::fs::read_to_string(&mkdocs).map_err(|why| MintRefused::Io(String::from("mkdocs.yml"), why))?;
    let listed = excluded(&text, &name).ok_or(MintRefused::NoExcludedRecord)?;
    let rel = format!("docs/adr/{name}");
    let words = slug.replace('-', " ");
    let mut title = words.get(..1).map_or_else(String::new, str::to_ascii_uppercase);
    title.push_str(words.get(1..).unwrap_or(""));
    let body = format!(
        "---\ntitle: {title}\ndescription: What this decides, and the issue it answers.\n---\n\n# {title}\n\n\
         Status: **proposed**.\n"
    );
    if let Err(why) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(&rel))
        .and_then(|mut file| file.write_all(body.as_bytes()))
    {
        return Err(MintRefused::Io(rel, why));
    }
    std::fs::write(&mkdocs, listed).map_err(|why| MintRefused::Io(String::from("mkdocs.yml"), why))?;
    Ok(rel)
}

/// `new-adr <slug>`: mint a record named by the current UTC second.
pub(crate) fn run(args: &[String]) -> Verdict {
    let [slug] = args else {
        eprintln!("usage: cargo xtask new-adr <slug>");
        return Verdict::Usage;
    };
    let Some(root) = crate::repo::root() else {
        eprintln!("xtask new-adr: no workspace root (a directory holding flake.nix and Cargo.toml)");
        return Verdict::Fail;
    };
    let Ok(since_epoch) = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
    else {
        eprintln!("xtask new-adr: the clock reads before 1970, so there is no second to name the record by");
        return Verdict::Fail;
    };
    match mint(&root, slug, Minted::at(since_epoch)) {
        Ok(rel) => {
            println!("xtask new-adr: minted {rel}, and listed it under exclude_docs in mkdocs.yml");
            Verdict::Pass
        }
        Err(why) => {
            eprintln!("xtask new-adr: {why}");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Id, MintRefused, Minted, Refused, mint};

    /// Epoch seconds against the second `python3`'s `datetime.fromtimestamp(s, timezone.utc)`
    /// names, including a 29 February and the last second of a year.
    #[test]
    fn a_second_since_the_epoch_is_the_utc_second_it_names() {
        for (since_epoch, named) in [
            (0, "19700101000000"),
            (951_782_400, "20000229000000"),
            (1_790_000_000, "20260921141320"),
            (4_107_542_399, "21000228235959"),
        ] {
            assert_eq!(Minted::at(since_epoch).to_string(), named);
            assert_eq!(Minted::parse(named), Some(Minted::at(since_epoch)), "{named}");
        }
    }

    #[test]
    fn a_minted_name_is_only_a_real_second() {
        for refused in [
            "20260230120000-no-thirtieth-of-february.md",
            "21000229000000-not-a-leap-year.md",
            "20261301000000-no-thirteenth-month.md",
            "20260923240000-no-hour-twenty-four.md",
            "2026092312000-thirteen-digits.md",
            "19691231235959-before-the-epoch.md",
        ] {
            assert_eq!(Id::of(refused), Err(Refused::NotASecond), "{refused}");
        }
    }

    #[test]
    fn mint_writes_the_record_and_lists_it_once() {
        // Two records, so "after the LAST one" is held, and an `adr/` line outside `exclude_docs`
        // after the block, so the search is held to that block.
        let tree = crate::scratch_tree::Tree::of(
            "new-adr",
            &[
                (
                    "mkdocs.yml",
                    b"exclude_docs: |\n  adr/0001-a.md\n  adr/0002-b.md\n  crap.md\n\nnav:\n  adr/elsewhere.md\n",
                ),
                ("docs/adr/0001-a.md", b"x\n"),
            ],
        );
        let at = Minted::at(1_790_000_000);
        let rel = mint(tree.root(), "a-decision", at).expect("a first mint");
        assert_eq!(rel, "docs/adr/20260921141320-a-decision.md");
        let record = std::fs::read_to_string(tree.root().join(&rel)).expect("the record");
        assert!(record.contains("# A decision\n"), "{record}");
        let listed = "exclude_docs: |\n  adr/0001-a.md\n  adr/0002-b.md\n  adr/20260921141320-a-decision.md\n  crap.md\n\n\
                      nav:\n  adr/elsewhere.md\n";
        let mkdocs = || std::fs::read_to_string(tree.root().join("mkdocs.yml")).expect("mkdocs.yml");
        assert_eq!(mkdocs(), listed);
        // The same second again is refused before mkdocs.yml is touched.
        let again = mint(tree.root(), "a-decision", at).unwrap_err();
        assert!(
            matches!(&again, MintRefused::Io(path, why) if *path == rel && why.kind() == std::io::ErrorKind::AlreadyExists),
            "{again:?}"
        );
        assert_eq!(mkdocs(), listed);
        for slug in ["", "Upper", "two--dashes", "-leading", "trailing-", "an_underscore"] {
            let refused = mint(tree.root(), slug, at).unwrap_err();
            assert!(matches!(refused, MintRefused::NotASlug(_)), "{slug:?}: {refused:?}");
        }
    }
}
