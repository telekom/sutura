//! The ordinal words the amendment sequence rule reads. The expected spellings are written out
//! here, independently of the reader's tables, so a cell cannot agree with the reader by sharing
//! its mistake.

use super::pages::{Heading, amendment, sequence_problems};

const UNITS: [&str; 9] = [
    "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
];
const TEENS: [&str; 10] = [
    "tenth",
    "eleventh",
    "twelfth",
    "thirteenth",
    "fourteenth",
    "fifteenth",
    "sixteenth",
    "seventeenth",
    "eighteenth",
    "nineteenth",
];
const TENS: [(&str, &str); 8] = [
    ("twenty", "twentieth"),
    ("thirty", "thirtieth"),
    ("forty", "fortieth"),
    ("fifty", "fiftieth"),
    ("sixty", "sixtieth"),
    ("seventy", "seventieth"),
    ("eighty", "eightieth"),
    ("ninety", "ninetieth"),
];

/// Every position from the first to the ninety-ninth with its spelling, in order.
fn spelled() -> Vec<(usize, String)> {
    let mut all: Vec<_> = UNITS
        .iter()
        .chain(&TEENS)
        .enumerate()
        .map(|(at, word)| (at + 1, (*word).to_owned()))
        .collect();
    for (tens, (cardinal, round)) in (20..).step_by(10).zip(TENS) {
        all.push((tens, round.to_owned()));
        all.extend(
            UNITS
                .iter()
                .enumerate()
                .map(|(at, unit)| (tens + at + 1, format!("{cardinal}-{unit}"))),
        );
    }
    all
}

/// A page of `count` amendments, each numbered as it should be.
fn page_of(count: usize) -> Vec<String> {
    spelled()
        .into_iter()
        .take(count)
        .map(|(_, word)| format!("## {word} amendment, 2026-10-07: x"))
        .collect()
}

#[test]
fn a_compound_ordinal_reads_as_the_position_it_names() {
    assert_eq!(
        amendment("## Twenty-first amendment, 2026-10-07: x"),
        Some(Heading::Numbered(21))
    );
    let all = spelled();
    assert_eq!(all.len(), 99);
    for (n, word) in all {
        let heading = format!("## {word} amendment");
        assert_eq!(amendment(&heading), Some(Heading::Numbered(n)), "{heading}");
        let shouted = heading.to_uppercase();
        assert_eq!(amendment(&shouted), Some(Heading::Numbered(n)), "{shouted}");
    }
}

#[test]
fn a_page_of_twenty_two_correctly_numbered_amendments_is_walked_whole() {
    let (problems, walked) = sequence_problems("a.md", &page_of(22));
    assert_eq!(walked, 22, "{problems:#?}");
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn a_wrong_ordinal_past_twenty_is_reported() {
    let mut lines = page_of(20);
    lines.push(String::from("## Twenty-second amendment, 2026-10-07: x"));
    let (problems, walked) = sequence_problems("a.md", &lines);
    assert_eq!(walked, 21);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].starts_with("a.md:21:"), "{problems:#?}");
    assert!(problems[0].contains("twenty-second"), "{problems:#?}");
}

#[test]
fn an_ordinal_the_reader_cannot_read_is_reported_rather_than_skipped() {
    let mut lines = page_of(99);
    lines.push(String::from("## Hundredth amendment, 2026-10-07: x"));
    assert!(amendment(&lines[99]).is_some(), "skipped as no amendment at all");
    let (problems, walked) = sequence_problems("a.md", &lines);
    assert_eq!(walked, 100);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(problems[0].starts_with("a.md:100:"), "{problems:#?}");
    assert!(!problems[0].contains("write it as"), "{problems:#?}");
    for word in ["Twenty-tenth", "Thirty-eleventh"] {
        let (reported, seen) = sequence_problems("a.md", &[format!("## {word} amendment")]);
        assert_eq!(seen, 1, "{word}");
        assert_eq!(reported.len(), 1, "{reported:#?}");
        assert!(reported[0].contains("cannot read"), "{reported:#?}");
    }
}

#[test]
fn a_wrong_ordinal_is_named_by_the_word_for_both_positions() {
    let all = spelled();
    // The heading at each position claims the next one, and the last claims the first.
    let claimed: Vec<&str> = all.iter().cycle().skip(1).take(99).map(|(_, word)| word.as_str()).collect();
    let lines: Vec<String> = claimed
        .iter()
        .map(|says| format!("## {says} amendment, 2026-10-07: x"))
        .collect();
    let (problems, walked) = sequence_problems("a.md", &lines);
    assert_eq!(walked, 99);
    assert_eq!(problems.len(), 99, "{problems:#?}");
    for (((n, named), says), problem) in all.iter().zip(&claimed).zip(&problems) {
        let expected = format!("a.md:{n}: reads as the {says} amendment and is the {named} in file order");
        assert!(problem.starts_with(&expected), "{problem}");
    }
}
