//! Every shipped-set declaration under `.github`, and what each one CARRIES.
//!
//! [`read`] is the whole literal rule up to the comparison: the walk, the classification, and the
//! refusals that stop a verdict being about them. The parent keeps the comparison and the report,
//! the way [`super::loops`] and [`super::refusal`] keep theirs.
//!
//! **Its own file for [`super::loops`]' reason: the parent is against the unexemptable 1000-line
//! cap.** Its fixtures are NEW tests and come with it, so nothing here can be orphaned by
//! reverting the parent.
//!
//! # The hole
//!
//! `is_literal_set` opened with `!value.is_empty()`, which is two answers under one word: *this is
//! not a declaration* and *there is nothing here to compare*. Only the second is true of a null,
//! and everything the predicate said no to was SKIPPED - neither compared against
//! `nix/shipped.nix` nor reported, so the printed literal count moved and no line said why.
//! Measured on this tree, `github.com/telekom/sutura#329`:
//!
//! ```text
//! ok - 4 literal(s) agree with nix/shipped.nix (sutura sutura-serve)   <- clean tree
//! ok - 3 literal(s) agree with nix/shipped.nix (sutura sutura-serve)   <- a null `default:`
//! ok - 3 literal(s) agree with nix/shipped.nix (sutura sutura-serve)   <- `BINARIES: ${{ env.SHIPPED }}`
//! ok - 4 literal(s) agree with nix/shipped.nix (sutura sutura-serve)   <- `with:` passing `binaries:`
//! ```
//!
//! All three mutations exit **0**. The first is a null `default:` under an action's `binaries:`
//! input, which [`super::loops`]' own empty rule does not reach - its null exemption is keyed on
//! `BINARIES:` alone, because `binaries:` with no value is how an input block OPENS. The second is
//! a literal replaced by an expression naming a DIFFERENT set, which is empty nowhere and so is
//! nothing that rule could ever have caught. **The third moved no number at all** - a null
//! argument was never a declaration to this scan, so it was invisible rather than subtracted, and
//! reading the count is no defence against it. A null `BINARIES:` in a workflow `env` is the
//! fourth and is red at exit 1 since #322 - from that sibling rule, while this one went on
//! dropping it behind the verdict. The verdict carries a second number now, for a reason that is
//! about none of these four - see [`declarations`].
//!
//! # The answer is a classification, not a predicate
//!
//! Every in-scope value is one of three things and none of them is *nothing*:
//!
//! * a **[`Carried::Set`]** - compared. **Zero names is a set**, so a null and a `""` disagree with
//!   `nix/shipped.nix` through the comparison that was already there, at the line they were
//!   spelled on.
//! * a **[`Carried::Reference`]** - one `${{ ... }}` naming this same set. There is genuinely
//!   nothing here to compare, so it is NAMED in the verdict instead of dropped.
//! * a **[`Carried::Opaque`]** - anything else. Refused, because a value the gate can neither
//!   compare nor attribute is exactly the state that moved the count in silence.
//!
//! # Why the scalar is lexed before it is classified
//!
//! Because [`Carried::Opaque`] FAILS, and a gate that fails a correct tree gets disabled. Both
//! `BINARIES: "sutura sutura-serve"` and a trailing `# comment` are legal YAML for the set this
//! compares, and both were silently dropped before - the defect above wearing a third face. So one
//! layer of matching quotes and a trailing comment come off first, which also settles
//! `BINARIES: # a comment` as the null it is.
//!
//! # What it does NOT reach
//!
//! * **Whether a reference RESOLVES, and this is the one limit that still lets the count move.**
//!   `${{ inputs.binaries }}` is accepted because its last segment names this set, not because
//!   anything followed it to a caller. So REPLACING a literal with a reference to this same set -
//!   `${{ steps.x.outputs.binaries }}` in place of `sutura sutura-serve` - is `ok - 3 literal(s)`
//!   at exit 0, the 4-to-3 drop this whole rule exists to close, surviving inside the arm the fix
//!   added. Measured in review. **It is held by a test and not by this gate**, deliberately: a
//!   reference is legitimate wherever a caller passes the set on, nothing here can resolve one,
//!   and a rule that refused them would fail a correct tree. `shipped::tests`'
//!   `the_real_tree_agrees_with_itself` pins the exact `file:line` of every literal AND every
//!   reference, so a literal that becomes a reference moves a row and `just test` goes red.
//! * **A block scalar** (`BINARIES: |`) reads as an [`Carried::Opaque`] `|` rather than as the
//!   lines below it, and [`spelled`] is line-oriented: a line inside a `run: |` body beginning
//!   `BINARIES:` would read as a declaration, and a quoted value spanning two lines truncates to
//!   the first. Neither shape exists under `.github` today - verified across all ten
//!   `BINARIES:` / `binaries:` lines - and both fail closed on this tree, so the cost is a false
//!   RED. That flips to a false GREEN only if `nix/shipped.nix` ever ships exactly one binary,
//!   which is the reason to write it down rather than to lex block scalars now.
//! * **Whether the RUNNER renders a null as an empty string.** Unmeasured - no runner to ask - so
//!   this rests on the declaration, as [`super::loops`] does: a key declared with no value is not
//!   a shipped set whatever it resolves to.

use std::collections::BTreeMap;

/// What one in-scope declaration of the shipped set carries.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Carried {
    /// A literal set of plain names, in declaration order. **An EMPTY one is a set of zero names
    /// and not an absence**, which is the whole of `github.com/telekom/sutura#329`.
    Set(Vec<String>),
    /// One `${{ ... }}` whose last segment names this same set, as the expression itself.
    Reference(String),
    /// A value that is neither, as the scalar that was read.
    Opaque(String),
}

/// The scalar half of a YAML value: one layer of matching quotes removed, or a trailing `#`
/// comment dropped.
///
/// A `#` opens a comment at the start of a scalar or after whitespace, which is the rule YAML
/// states and the reason `sutura-serve#1` would not be truncated. An unterminated quote yields the
/// rest of the value rather than nothing, so a typo is classified rather than silently emptied.
fn scalar(value: &str) -> &str {
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let rest = value.get(1..).unwrap_or_default();
        return rest.find(quote).and_then(|end| rest.get(..end)).unwrap_or(rest);
    }
    let mut after_space = true;
    for (at, character) in value.char_indices() {
        if character == '#' && after_space {
            return value.get(..at).unwrap_or(value).trim();
        }
        after_space = character.is_whitespace();
    }
    value
}

/// Every spelling of the YAML null, which all mean the same thing and were read three different
/// ways until review measured it.
///
/// `""` gave `Set([])`, the bare word `null` gave `Set(["null"])` - a shipped binary called
/// *null* - and `~` gave an `Opaque` refusal. One value, three verdicts. They are the empty set
/// now, which is what YAML says they are and what `""` already was.
const NULLS: [&str; 4] = ["~", "null", "Null", "NULL"];

/// Does this value declare NOTHING at all - the shape that opens an action's input block?
///
/// The distinction the parent needs and the classification deliberately does not make: `binaries:`
/// with NO VALUE opens a block, while `binaries: ""` and `binaries: ~` declare the empty set. A
/// quoted value is never null however empty its interior is, and neither is an EXPLICIT null: a
/// key spelled `~` carries a value, so nothing is nested under it.
pub(super) fn is_null(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.starts_with(['"', '\'']) && scalar(trimmed).is_empty()
}

/// One `${{ ... }}` and nothing else, as its interior.
///
/// A value carrying two of them, or an expression with text around it, is not a reference to one
/// set - it is [`Carried::Opaque`], which is a verdict rather than a guess.
fn expression(value: &str) -> Option<&str> {
    let inner = value.strip_prefix("${{")?.strip_suffix("}}")?.trim();
    (!inner.contains("}}") && !inner.contains("${{")).then_some(inner)
}

/// Is this word a plain name rather than part of an expression?
fn is_plain_name(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Classify one in-scope value. Every value is one of the three, so nothing is skipped.
pub(super) fn carried(value: &str) -> Carried {
    let value = scalar(value);
    if let Some(expression) = expression(value) {
        // The LAST segment, so `env.BINARIES` and `inputs.binaries` are both this set and
        // `env.SHIPPED` is not. Case-insensitive because the two keys differ only in case.
        let named = expression.rsplit('.').next().unwrap_or(expression).trim();
        return if named.eq_ignore_ascii_case(super::KEYS[0]) {
            Carried::Reference(String::from(expression))
        } else {
            Carried::Opaque(String::from(value))
        };
    }
    if NULLS.contains(&value) {
        return Carried::Set(Vec::new());
    }
    if value.split_whitespace().all(is_plain_name) {
        Carried::Set(value.split_whitespace().map(String::from).collect())
    } else {
        Carried::Opaque(String::from(value))
    }
}

/// One declaration of the shipped set, with the line it was spelled on and what it carries.
///
/// **EVERY in-scope declaration produces one of these**, which is the difference #329 was about:
/// a value the gate could not compare used to produce nothing at all, so the count it prints moved
/// with no line saying which declaration had stopped being read. [`Carried`] is the
/// classification and carries the reasoning.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Spelled {
    pub(super) line: usize,
    pub(super) carries: Carried,
}

/// Every declaration of the shipped-binary set one YAML file makes.
///
/// Two shapes, because a workflow and a composite action declare the same thing differently:
///
/// * `BINARIES: sutura sutura-serve` - a workflow's `env`, at any indent.
/// * an input called `binaries:` whose block carries `default: sutura sutura-serve` - an action's
///   input. The block is the lines indented deeper than the key, which is the only thing about
///   YAML this needs to know.
///
/// **A NULL OPENS A BLOCK ONLY UNDER THE INPUT KEY**, and that split is the parent half of #329.
/// `binaries:` with no value is how an input block opens; `BINARIES:` with no value opens nothing
/// and declares the empty set. A block that closes having contained no deeper line at all was a
/// null argument rather than an input declaration, so it is the empty set too - `with:` passing
/// `binaries:` and nothing else.
pub(super) fn spelled(text: &str) -> Vec<Spelled> {
    let mut found = Vec::new();
    let mut block: Option<Block> = None;
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let indent = line.len().saturating_sub(line.trim_start().len());
        let at = index.saturating_add(1);

        if let Some(open) = &mut block {
            if trimmed.is_empty() {
                continue;
            }
            if indent > open.indent {
                if let Some(value) = trimmed.strip_prefix("default:") {
                    found.push(Spelled {
                        line: at,
                        carries: carried(value),
                    });
                    block = None;
                } else {
                    open.nested = true;
                }
                continue;
            }
            found.extend(open.closed());
            block = None;
        }

        for key in super::KEYS {
            let Some(rest) = trimmed.strip_prefix(key) else {
                continue;
            };
            let Some(value) = rest.strip_prefix(':') else {
                continue;
            };
            if key == super::KEYS[1] && is_null(value) {
                block = Some(Block {
                    indent,
                    line: at,
                    nested: false,
                });
            } else {
                found.push(Spelled {
                    line: at,
                    carries: carried(value),
                });
            }
            break;
        }
    }
    // A file ending inside the block declared nothing under it either.
    found.extend(block.as_ref().and_then(Block::closed));
    found
}

/// Every declaration across a set of files, as `(file, what it carries)`, and the files the walk
/// actually INSPECTED.
///
/// **The second return is half of a pair taken from two DIFFERENT places** - the file finder
/// produces `files`, the walk produces this - because a verdict over a scan that stopped early
/// reads exactly like one over the whole tree, and a row total cannot tell them apart: a file that
/// declares nothing contributes no row either way. `check-api-links`' `scanned == pages.len()` is
/// the same rule, and `.agents/skills/sutura/gates/SKILL.md` records what one total cost there.
///
/// NAMES rather than a count, so the caller can say WHICH file went unread and so the witness
/// cannot be satisfied by assigning the finder's own number to a variable.
///
/// **THE NAME IS RECORDED AFTER [`spelled`] RETURNS, and that ordering is the whole witness.**
/// The first version pushed it first and independently of the parse, so the list said a file had
/// been OFFERED to the walk rather than read out of - and a `continue` between the two left both
/// composite actions unread while the verdict still said *across 12 file(s)*, at exit 0, with the
/// whole suite green. Measured in review with `if name.contains("/actions/") { continue; }`:
/// `ok - 2 literal(s) across 12 file(s)`. **A witness written before the work it attests to is not
/// a witness**, which is `.agents/skills/sutura/gates/SKILL.md`'s *a readout under a comment
/// claiming it is an assertion*, one shape over.
///
/// What it still does not reach is the FINDER: a file `super::yaml_files` never handed over is not
/// in `files` either, so this comparison holds trivially over it. That arm is fail-closed there.
pub(super) fn declarations(files: &BTreeMap<String, String>) -> Walk<'_> {
    let mut walk = Walk {
        rows: Vec::new(),
        inspected: Vec::new(),
    };
    for (name, text) in files {
        let found = spelled(text);
        walk.rows.extend(found.into_iter().map(|found| (name.as_str(), found)));
        walk.inspected.push(name.as_str());
    }
    walk
}

/// What one walk over the `.github` files produced. A named struct rather than a tuple, for
/// [`Block`]'s reason: `clippy::type_complexity` refuses the tuple and is right to.
pub(super) struct Walk<'a> {
    /// Every declaration, as `(file, what it carries)`.
    pub(super) rows: Vec<(&'a str, Spelled)>,
    /// The files this walk actually read, in walk order.
    pub(super) inspected: Vec<&'a str>,
}

/// An open `binaries:` block: where it started, and whether anything is nested under it.
///
/// A named struct rather than a tuple, for [`super::Reconciliation`]'s reason one file over:
/// `clippy::type_complexity` refuses the tuple and is right to, because two positional `usize`s
/// say nothing about which is the indent and which is the line.
struct Block {
    indent: usize,
    line: usize,
    nested: bool,
}

impl Block {
    /// What this block declared, now that it has closed without a `default:`.
    ///
    /// A block with a BODY is an input declaration that states no default, which is out of scope.
    /// One with nothing under it was never a declaration at all - it is `with:` passing the key
    /// and no value, so it declares the empty set at its own line.
    fn closed(&self) -> Option<Spelled> {
        (!self.nested).then(|| Spelled {
            line: self.line,
            carries: Carried::Set(Vec::new()),
        })
    }
}

/// One literal set that disagrees with `nix/shipped.nix`. A named struct rather than a tuple,
/// for [`super::Reconciliation`]'s reason: `clippy::type_complexity` refuses the tuple.
#[derive(Debug)]
pub(super) struct Mismatch {
    /// `file:line`, so the row a reader acts on names where it was spelled.
    pub(super) at: String,
    /// What it spells. EMPTY IS A VALUE here - see [`Carried::Set`].
    pub(super) spells: Vec<String>,
}

/// Every declaration in `.github`, sorted into the buckets the verdict is built from.
pub(super) struct Read {
    /// One row per literal set, compared. Its length is the printed count.
    pub(super) literals: Vec<String>,
    /// One row per reference to this same set. Compared to nothing by design, and NAMED so the
    /// count is not the only thing that moves when a declaration stops being compared.
    pub(super) references: Vec<String>,
    /// The literal sets that disagree with `nix/shipped.nix`.
    pub(super) mismatches: Vec<Mismatch>,
    /// How many files the walk read - the second half of the pair, see
    /// [`declarations`].
    pub(super) inspected: usize,
}

/// Read and classify every declaration, or print the refusal that stops the verdict being about
/// them and answer `None`.
///
/// Its own function because `run` is against `clippy::too_many_lines`, and this is the half of it
/// `github.com/telekom/sutura#329` rewrote.
pub(super) fn read(files: &BTreeMap<String, String>, expected: &[String]) -> Option<Read> {
    // TWO NUMBERS FROM TWO PLACES: the walk reports which files it read and the file finder which
    // it found, so a scan that stopped early is a verdict rather than a smaller count. A row total
    // cannot stand in for it - a file declaring nothing contributes no row whether it was read or
    // not - which is `check-api-links`' `scanned == pages.len()` one gate over.
    let walk = declarations(files);
    let unread: Vec<&str> = files
        .keys()
        .map(String::as_str)
        .filter(|name| !walk.inspected.contains(name))
        .collect();
    if !unread.is_empty() {
        eprintln!(
            "xtask check-shipped-binaries: FAILED - the walk read {} of the {} file(s) under `.github`",
            walk.inspected.len(),
            files.len()
        );
        for name in &unread {
            eprintln!("  {name} was found and not inspected");
        }
        eprintln!();
        eprintln!("  A verdict over a scan that stopped early reads exactly like one over the whole");
        eprintln!("  tree, and the rows below it would look no different: a file that declares nothing");
        eprintln!("  contributes none either way.");
        return None;
    }

    // ROWS RATHER THAN A COUNT: the count was the only thing #329's shapes could move, and one of
    // them did not move it either. Every declaration lands in exactly one of the three.
    let mut read = Read {
        literals: Vec::new(),
        references: Vec::new(),
        mismatches: Vec::new(),
        inspected: walk.inspected.len(),
    };
    let mut opaque: Vec<String> = Vec::new();
    for (name, set) in walk.rows {
        let at = format!("{name}:{}", set.line);
        match set.carries {
            Carried::Set(names) => {
                if names != expected {
                    read.mismatches.push(Mismatch {
                        at: at.clone(),
                        spells: names,
                    });
                }
                read.literals.push(at);
            }
            Carried::Reference(expression) => read.references.push(format!("{at} -> {expression}")),
            Carried::Opaque(value) => opaque.push(format!("{at} carries `{value}`")),
        }
    }

    if !opaque.is_empty() {
        eprintln!(
            "xtask check-shipped-binaries: FAILED - {} declaration(s) this gate can neither compare nor attribute",
            opaque.len()
        );
        for row in &opaque {
            eprintln!("  {row}");
        }
        eprintln!();
        eprintln!("  Neither a literal set nor one expression naming this same set, which used to be");
        eprintln!("  SKIPPED: the printed count moved and no line said which comparison had stopped -");
        eprintln!("  `github.com/telekom/sutura#329`. Spell the set out, or reference it under a name");
        eprintln!("  whose last segment is `{}`.", super::KEYS[1]);
        return None;
    }

    if read.literals.is_empty() {
        eprintln!("xtask check-shipped-binaries: FAILED - no shipped-binary literal in any workflow or action");
        eprintln!(
            "  Every path that builds the shipped set spells it again: `{}` in a workflow",
            super::KEYS[0]
        );
        eprintln!(
            "  `env`, `{}` as a build action's input default. Finding none across the {}",
            super::KEYS[1],
            files.len()
        );
        eprintln!("  file(s) under `.github/workflows` and `.github/actions` means this gate is");
        eprintln!("  reading nothing rather than that the literals agree.");
        return None;
    }
    Some(read)
}

#[cfg(test)]
mod tests {
    use super::Carried;

    #[test]
    fn a_value_that_declares_nothing_is_the_empty_set_and_not_an_absence() {
        // #329: the early `is_empty` skipped these, so the comparison stopped and the count moved
        // with nothing saying so. Zero names is a set, and it disagrees with what nix ships.
        assert_eq!(super::carried(""), Carried::Set(Vec::new()));
        assert_eq!(super::carried("  "), Carried::Set(Vec::new()));
        assert_eq!(super::carried("\"\""), Carried::Set(Vec::new()));
        assert_eq!(super::carried("''"), Carried::Set(Vec::new()));
        assert_eq!(super::carried("# nothing here"), Carried::Set(Vec::new()));
    }

    #[test]
    fn the_yaml_null_is_read_one_way_however_it_is_spelled() {
        // Measured in review: ONE value, THREE verdicts. `""` was the empty set, the bare word
        // `null` was a shipped binary CALLED `null`, and `~` was an `Opaque` refusal that would
        // have failed a correct tree. YAML says all of them are the same thing, and so does the
        // absent value one test down, so all of them are the empty set.
        for spelling in ["", "  ", "\"\"", "''", "~", "null", "Null", "NULL", "# nothing here"] {
            assert_eq!(
                super::carried(spelling),
                Carried::Set(Vec::new()),
                "`{spelling}` is a YAML null and has to read as the empty set"
            );
        }
        // And an explicit null is a VALUE, so it does not open an input block the way an absent
        // one does - nothing can be nested under a key that already carries `~`.
        assert!(!super::is_null("~"), "an explicit null carries a value");
        assert!(!super::is_null("null"));
    }

    #[test]
    fn a_null_opens_an_input_block_and_an_explicitly_empty_value_does_not() {
        // The one distinction the parent still needs: `binaries:` with no value is how an action's
        // input block OPENS, so reading it as an empty set would fail every correct action.
        assert!(super::is_null(""));
        assert!(super::is_null("  # a comment"));
        assert!(!super::is_null("\"\""), "an explicit empty string declares the empty set");
        assert!(!super::is_null("''"));
        assert!(!super::is_null("sutura"));
    }

    #[test]
    fn a_reference_names_this_set_or_it_is_not_a_reference() {
        // MEASURED as a live silent drop: `BINARIES: ${{ env.SHIPPED }}` in place of the literal
        // printed `ok - 3 literal(s)` and exit 0 on this tree. An expression is empty nowhere, so
        // no empty-set rule could ever have caught it.
        assert_eq!(
            super::carried("${{ env.BINARIES }}"),
            Carried::Reference(String::from("env.BINARIES"))
        );
        assert_eq!(
            super::carried("${{ inputs.binaries }}"),
            Carried::Reference(String::from("inputs.binaries"))
        );
        assert_eq!(
            super::carried("${{ env.SHIPPED }}"),
            Carried::Opaque(String::from("${{ env.SHIPPED }}"))
        );
        // Two expressions are not a reference to one set, and neither is one with text beside it.
        assert!(matches!(
            super::carried("${{ env.BINARIES }} ${{ env.BINARIES }}"),
            Carried::Opaque(_)
        ));
        assert!(matches!(super::carried("sutura ${{ env.BINARIES }}"), Carried::Opaque(_)));
    }

    /// The three files the `read` tests below walk: a literal, an action input, and a page that
    /// declares nothing. Built per test so nothing is shared between them.
    fn tree(default_for_the_action: &str) -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([
            (
                String::from(".github/workflows/release.yml"),
                String::from("env:\n  BINARIES: sutura sutura-serve\n"),
            ),
            (
                String::from(".github/actions/build/action.yml"),
                format!("inputs:\n  binaries:\n    required: false\n    default:{default_for_the_action}\n"),
            ),
            (
                String::from(".github/workflows/docs.yml"),
                String::from("jobs:\n  build:\n    steps: []\n"),
            ),
        ])
    }

    #[test]
    fn read_sorts_every_declaration_into_the_bucket_the_verdict_is_built_from() {
        // THE CONSUMER, not the classification. Every other test here asserts on `spelled` and
        // `carried`, and review measured what that leaves open: one arm added to the match inside
        // `read` - `Carried::Set(names) if names.is_empty() => {}` - restored the pre-#329 silent
        // green BYTE FOR BYTE at exit 0, with all of those tests still passing. This is where a
        // classification becomes a mismatch, a row and an exit code, so this is where it is held.
        let expected = [String::from("sutura"), String::from("sutura-serve")];
        let read = super::read(&tree(" sutura sutura-serve"), &expected).expect("a clean tree is not a refusal");
        assert_eq!(
            read.literals,
            [".github/actions/build/action.yml:4", ".github/workflows/release.yml:2"]
        );
        assert!(read.mismatches.is_empty(), "{:?}", read.mismatches);
        // Three files walked, two of which declare something: the pair, at the level `run` uses.
        assert_eq!(read.inspected, 3);
    }

    #[test]
    fn read_turns_a_declaration_carrying_nothing_into_a_mismatch_the_verdict_names() {
        // Shape A of `github.com/telekom/sutura#329`, driven through the consumer: a null
        // `default:` under an action's `binaries:` input. It has to arrive as a MISMATCH carrying
        // zero names at its own line, because that is what makes `run` exit 1 - and it is exactly
        // what an `is_empty` arm takes away while every classification test stays green.
        let expected = [String::from("sutura"), String::from("sutura-serve")];
        let read = super::read(&tree(""), &expected).expect("an empty declaration is a mismatch, not a refusal");
        assert_eq!(read.mismatches.len(), 1, "{:?}", read.mismatches);
        assert_eq!(read.mismatches[0].at, ".github/actions/build/action.yml:4");
        assert!(read.mismatches[0].spells.is_empty(), "{:?}", read.mismatches[0].spells);
        // And it is still COUNTED - the count must not drop, which is the whole of #329.
        assert_eq!(read.literals.len(), 2, "{:?}", read.literals);
    }

    #[test]
    fn read_refuses_a_value_it_can_neither_compare_nor_attribute() {
        // Shape B: an expression naming a DIFFERENT set. `None` is the refusal `run` turns into
        // exit 1, and it must not arrive as a quietly shorter literal list.
        let expected = [String::from("sutura"), String::from("sutura-serve")];
        assert!(super::read(&tree(" ${{ env.SHIPPED }}"), &expected).is_none());
        // A reference to THIS set is not that, and is named rather than dropped.
        let read = super::read(&tree(" ${{ env.BINARIES }}"), &expected).expect("a reference is not a refusal");
        assert_eq!(read.references, [".github/actions/build/action.yml:4 -> env.BINARIES"]);
        assert_eq!(read.literals, [".github/workflows/release.yml:2"]);
    }

    #[test]
    fn every_file_handed_to_the_walk_is_inspected_and_a_second_list_says_so() {
        // THE PAIR, from two different places: this list comes from the WALK and the parent
        // compares it against the file finder's own keys. A row total cannot stand in for it - the
        // middle file here declares nothing and contributes no row, so a walk that skipped it
        // would produce the same two rows as one that read it.
        let files = std::collections::BTreeMap::from([
            (String::from("a.yml"), String::from("env:\n  BINARIES: sutura\n")),
            (String::from("b.yml"), String::from("# this file declares nothing\n")),
            (String::from("c.yml"), String::from("env:\n  BINARIES: sutura-serve\n")),
        ]);
        let walk = super::declarations(&files);
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(walk.inspected, names, "{:?}", walk.rows);
        assert_eq!(walk.rows.len(), 2, "{:?}", walk.rows);
        assert_eq!(walk.rows[0].0, "a.yml");
        assert_eq!(walk.rows[1].0, "c.yml");
    }

    #[test]
    fn a_quoted_literal_and_a_trailing_comment_are_still_the_literal() {
        // LOAD-BEARING because `Opaque` fails: both shapes are legal YAML for this set, both were
        // silently dropped before, and a gate that reddens a correct tree gets disabled.
        let expected = Carried::Set(vec![String::from("sutura"), String::from("sutura-serve")]);
        assert_eq!(super::carried("sutura sutura-serve"), expected);
        assert_eq!(super::carried("\"sutura sutura-serve\""), expected);
        assert_eq!(super::carried("'sutura sutura-serve'"), expected);
        assert_eq!(super::carried("sutura sutura-serve # two of them"), expected);
        // A `#` that opens no comment, so a name is never truncated into a different name.
        assert_eq!(super::carried("sutura#1"), Carried::Opaque(String::from("sutura#1")));
    }
}
