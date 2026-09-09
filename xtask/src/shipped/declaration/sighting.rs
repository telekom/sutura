//! The FLOOR: every line that spells this set's key, counted by a predicate the parse cannot
//! narrow.
//!
//! **Its own file because a floor computed inside the thing it polices is not a floor**, and this
//! one was measured failing that way. Its first version lived inside [`super::spelled`]'s return
//! value, and a mutation handing that parse an empty body took the floor away with it - back to
//! `ok - 2 literal(s) across 12 file(s)` at exit 0 with both composite actions unread. The
//! subtraction happens at [`super::declarations`]' call site off the same `text` now, and the
//! sighting lives here, where nothing in the parse can reach it.
//! `github.com/telekom/sutura#414`.
//!
//! The parent was also against the unexemptable 1000-line cap, and this is the seam it already
//! argues for in prose: the OTHER predicate. Its tests are NEW and come with it.

/// Every line a substring search sees this set's key in KEY POSITION, 1-based.
///
/// **THE FLOOR, TAKEN BEFORE THE KEY READER GETS THE LINE**, which is `workflows::reach::scan`'s
/// `sighted` list one gate over and exists for the same reason: a spelling the reader does not
/// recognise has to COUNT, or it is invisible rather than uncounted - and a shape that was never
/// counted is out of reach of the row floor as well. `github.com/telekom/sutura#414`.
///
/// BROADER than [`super::spelled`]'s own match in four ways, each of them a declaration that would
/// otherwise be neither compared, reported nor counted: any number of `- ` sequence markers
/// (`- binaries: ...` is how a `strategy.matrix` entry spells this set, the shape `release.yml`
/// names in prose, and it was measured invisible at exit 0 with a verdict byte-identical to a
/// clean tree's); a quoted key; whitespace before the colon; and any case.
///
/// NARROWER than a bare `contains`, and that is measured rather than cautious: `.github` holds
/// seven prose mentions of the word today - `attest-and-sign/action.yml:16` and
/// `release-performance.yml:19` among them - so a floor that sighted those would redden a correct
/// tree, which is how a gate gets disabled. A comment line is not a declaration either.
pub(super) fn sighted(text: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut head = line.trim();
        if head.starts_with('#') {
            continue;
        }
        // ANY number of markers: `- - binaries:` is a nested sequence item, and one strip is
        // precisely what cannot see it.
        while let Some(rest) = head.strip_prefix('-').and_then(|rest| rest.strip_prefix([' ', '\t'])) {
            head = rest.trim_start();
        }
        let head = head.trim_start_matches(['"', '\'']);
        let after = super::super::KEYS.iter().find_map(|key| {
            let start = head.get(..key.len())?;
            start.eq_ignore_ascii_case(key).then(|| head.get(key.len()..)).flatten()
        });
        let Some(after) = after else {
            continue;
        };
        if after.trim_start_matches(['"', '\'']).trim_start().starts_with(':') {
            out.push(index.saturating_add(1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{Spellings, declarations, spelled};
    use super::sighted;

    #[test]
    fn a_walk_that_keeps_the_name_and_drops_the_parse_is_still_a_verdict() {
        // WHAT THE FILE-NAME PAIR CANNOT REACH, and the reason a second predicate exists. The two
        // sides of `inspected == offered` are both keyed on the NAME, so a mutation that keeps the
        // name in the witness and hands the parse nothing satisfies both. The substring sighting
        // does not: it reads the file's TEXT, so the key is seen whatever the parse did with it.
        let files = std::collections::BTreeMap::from([(
            String::from(".github/workflows/a.yml"),
            String::from("env:\n  BINARIES: sutura\n"),
        )]);
        // The mutation, spelled here rather than described: the name enters, the parse returns
        // nothing. The floor is subtracted at the call site from the same `text`, so it survives.
        let text = &files[".github/workflows/a.yml"];
        let dropped = Spellings {
            found: Vec::new(),
            accounted: Vec::new(),
        };
        assert!(dropped.found.is_empty(), "the dropped parse leaves no found spellings");
        let unaccounted: Vec<usize> = sighted(text)
            .into_iter()
            .filter(|line| !dropped.accounted.contains(line))
            .collect();
        assert_eq!(unaccounted, vec![2], "the sighting has to survive an emptied parse");
        // And on the real walk there is nothing left over, because the parse read it.
        let walk = declarations(&files);
        assert_eq!(walk.rows.len(), 1, "{:?}", walk.rows);
        assert!(walk.unaccounted.is_empty(), "{:?}", walk.unaccounted);
    }

    #[test]
    fn a_key_the_reader_does_not_recognise_is_counted_rather_than_dropped() {
        // THE FLOOR FROM THE OTHER PREDICATE. Each of these spells the key in key position and is
        // a shape the reader does not classify, so each has to arrive as an unaccounted LINE - a
        // declaration nobody compared is invisible rather than uncounted otherwise, which is what
        // put the sequence-item shape out of reach of every count in the verdict.
        for yaml in [
            "  \"binaries\": sutura\n", // a quoted key
            "  binaries : sutura\n",    // a space before the colon
            "  - - binaries: sutura\n", // a nested sequence item
            "  Binaries: sutura\n",     // a case neither key spells
        ] {
            let spellings = spelled(yaml);
            assert!(spellings.found.is_empty(), "{yaml:?} was classified: {spellings:?}");
            assert!(spellings.accounted.is_empty(), "{yaml:?}: {spellings:?}");
            assert_eq!(sighted(yaml), vec![1], "{yaml:?} went uncounted");
        }
        // And prose is NOT sighted, because a floor that reddens a correct tree gets disabled:
        // `.github` holds seven mentions of the word today and none of them declares anything.
        for prose in [
            "  # how many binaries ship. A `list`'s name is\n",
            "      # many binaries ship: `release.yml` writes\n",
            "            echo \"## Binaries\"\n",
            "      - name: Say where the binaries went\n",
        ] {
            assert!(sighted(prose).is_empty(), "{prose:?} was sighted as a declaration");
        }
    }
}
