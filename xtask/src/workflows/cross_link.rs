//! The 2/4 cross-matrix split is held here, not by the inline ternary that spells it.
//!
//! `cross-link.yml`'s `link` job builds two aarch64 targets on a `pull_request` and all four on a
//! push to `main`, and until this module the only thing that *chose* was a single inline
//! `${{ A && B || C }}` on `strategy.matrix.target` that no structural gate read. `check-workflows`,
//! `check-shipped-binaries` and `max-lines` all stayed green on a 4-on-PR regression, because they
//! ask whether references resolve, whether the shipped set agrees, and how long a file is - none of
//! which is *which set an event selects*. The PR-2/main-4 claim that accompanies that literal was
//! therefore a rule with no mechanism (AGENTS.md calls it a wish).
//!
//! This module anchors the ternary: it locates `jobs.link.strategy.matrix.target`, requires the one
//! `${{ }}`, splits it at top-level `&&`/`||` into the exact three operands `A && B || C`, pins `A`
//! token-for-token to `github.event_name == 'pull_request'`, resolves `B`/`C` through the same
//! `serde_json` parse the workflow's `fromJSON` does, and pins both leg lists order-sensitively.
//! It rides the existing `check-workflows` gate - no new just task, no Task row, no gate-count
//! change - and is covered by `just hygiene` like `sast`.
//!
//! # What this holds
//!
//! * **One event runs one set.** A plain scalar list (the pre-#477 shape) or any `${{ }}` that is
//!   not the exact `A && B || C` is refused, because such a shape lets a PR run the full four.
//! * **Which set each event gets, and what is in it.** The `pull_request` leg pins to the aarch64
//!   pair and the fallback to all four, both order-sensitively, so a third PR leg, a reordered
//!   pair, a partial all-four, an inverted ternary or a swapped event all redden.
//! * **The predicate is exactly the one the caller's event produces.** The `github` context of a
//!   called workflow reflects the CALLER's event; pinning the token string refuses a rewrite to
//!   `push` or `merge_group` or an inverted `!=`.
//!
//! # What this does not hold
//!
//! * **Not whether Actions actually run 2 vs 4.** Executing the matrix is runtime behaviour, not a
//!   string in this tree; this gate holds the literal that selects the set, and the header comment
//!   next to it stays honest only if the two stay the same document.
//! * **Not the `ci.yml` caller's `cross` job condition** (`if:` / `on:`), which decides *when* the
//!   called workflow runs at all, nor whether the legs are required contexts (`contexts` owns
//!   that). A caller could drop the `cross` job entirely and this matrix literal would stay green -
//!   the gate holds the matrix, not the invitation to it.

use std::path::Path;

/// Where the workflows live, read directly (not through [`super::sources`]) exactly as `sast` does.
const WORKFLOWS: &str = ".github/workflows";

/// The file that owns the event-scoped cross matrix.
const FILE: &str = "cross-link.yml";

/// The predicate the caller's event must select on - the only value that means "pull request".
///
/// Pinned token-for-token (equality, not `contains`), so `push`, `merge_group`, or an inverted
/// `!= 'pull_request'` all fail the pin.
const PREDICATE: &str = "github.event_name == 'pull_request'";

/// The two aarch64 triples a pull request proves. This is the only cross surface the native `ci`
/// job never compiles, so a branch re-pays nothing for the host triple.
const PR_PAIR: &[&str] = &["aarch64-unknown-linux-gnu", "aarch64-unknown-linux-musl"];

/// The full four-triple set a push to `main` runs. Deliberately the same list as `release.yml`'s
/// `build`. Order matters and is pinned: the canonical spelling is what a reader and the cache
/// priming expect.
const ALL_FOUR: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
];

/// Every way the cross-matrix split can have stopped being the 2/4 one it claims to be.
pub(super) fn problems(root: &Path) -> Vec<String> {
    let path = root.join(WORKFLOWS).join(FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => return vec![format!("{FILE} could not be read: {error}")],
    };

    let Some((line, value)) = cross_matrix_target(&text) else {
        return vec![format!(
            "{FILE}: no `jobs.link.strategy.matrix.target` found - the cross matrix is gone"
        )];
    };

    let mut found = Vec::new();
    if let Err(reason) = holds_the_split(&value) {
        found.push(format!("{FILE}:{line}: {reason}"));
    }
    found
}

/// Locate `jobs.link.strategy.matrix.target` and return its line number and raw value.
///
/// The `link` job's `matrix:` block has exactly one child key, `target:`; the later
/// `target: ${{ matrix.target }}` (line ~259) is a *reference* back into this matrix inside a
/// later job's `with:`, not a second matrix. The block is identified by taking the first `target:`
/// line that follows the file's first `matrix:` line - which is the cross matrix in production and
/// in every refusal fixture.
fn cross_matrix_target(text: &str) -> Option<(usize, String)> {
    let in_matrix = text.lines().position(|line| line.trim() == "matrix:");
    let start = in_matrix.map(|i| i + 1)?;
    for (offset, later) in text.lines().enumerate().skip(start) {
        if let Some(rest) = later.trim_start().strip_prefix("target:") {
            return Some((offset + 1, rest.trim().to_owned()));
        }
    }
    None
}

/// Assert `value` is the exact `${{ A && B || C }}`, with pinned predicate and pinned sets.
fn holds_the_split(value: &str) -> Result<(), String> {
    let value = value.trim();
    // A plain scalar list (the pre-#477 shape) or any bare set has no event selection at all.
    let inner = value
        .strip_prefix("${{")
        .and_then(|v| v.strip_suffix("}}"))
        .map(str::trim)
        .ok_or_else(|| {
            "the target is not a single `${{ }}` expression - a plain scalar list lets one event \
             run both sets"
                .to_owned()
        })?;

    let split = split_top_level(inner);
    let [a, b, c] = split.operands.as_slice() else {
        return Err(format!(
            "expected exactly the three operands `A && B || C`, found {}",
            split.operands.len()
        ));
    };
    if !matches!(split.operators.as_slice(), ["&&", "||"]) {
        return Err(format!(
            "expected the operators `&&` then `||` in that order, found {}",
            split.operators.join("` then `")
        ));
    }

    let a = a.trim();
    if a != PREDICATE {
        return Err(format!(
            "the pull-request predicate is not exactly `{PREDICATE}` - found `{a}`"
        ));
    }

    let b = parse_leg_set(b)?;
    let c = parse_leg_set(c)?;

    if b != PR_PAIR {
        return Err(format!(
            "the pull-request leg is not exactly the two-terminal aarch64 pair - found {b:?}"
        ));
    }
    if c != ALL_FOUR {
        return Err(format!(
            "the fallback leg is not exactly the four-term all-four list - found {c:?}"
        ));
    }
    Ok(())
}

/// Resolve a `fromJSON('<json>')` operand into its ordered leg list, or refuse it.
fn parse_leg_set(operand: &str) -> Result<Vec<String>, String> {
    let operand = operand.trim();
    let rest = operand
        .strip_prefix("fromJSON(")
        .ok_or_else(|| format!("the leg `{operand}` is not a fromJSON(...) call - out of position"))?;
    let rest = rest
        .strip_suffix(')')
        .ok_or_else(|| format!("the fromJSON for `{operand}` does not close with `)`"))?;
    let json = rest
        .trim()
        .strip_prefix('\'')
        .and_then(|r| r.strip_suffix('\''))
        .ok_or_else(|| format!("the fromJSON argument for `{operand}` is not a single-quoted string"))?;
    serde_json::from_str(json).map_err(|error| format!("unparseable JSON in fromJSON: {error}"))
}

/// Split at top-level `&&` / `||`, tracking `fromJSON` parentheses and single-quoted literals so a
/// separator inside a leg's JSON string is never mistaken for a ternary operator. Returns one
/// operand per top-level operator run, plus the operators between them.
///
/// A struct rather than a tuple so `-D clippy::type-complexity` stays quiet; operands are owned
/// because the expression is rebuilt character by character instead of sliced, which is what keeps
/// `-D clippy::string-slice` off an ASCII-only grammar.
struct SplitParts {
    operands: Vec<String>,
    operators: Vec<&'static str>,
}

/// The operands of `expr` split at top-level `&&` / `||`, in order, with the operators between
/// them. Quotes and `(...)` are tracked so a `&&`/`||` inside a leg's JSON string or a `fromJSON`
/// argument is not treated as a separator.
fn split_top_level(expr: &str) -> SplitParts {
    let mut operands = Vec::new();
    let mut operators = Vec::new();
    let bytes = expr.as_bytes();
    let mut depth = 0usize;
    let mut in_quote = false;
    let mut current = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(&b) = bytes.get(i) else { break };
        // `&&`/`||` only at the top level: outside quotes and outside `(...)`.
        let operator = if depth == 0 && !in_quote && i + 1 < bytes.len() {
            if b == b'&' && bytes.get(i + 1) == Some(&b'&') {
                Some("&&")
            } else if b == b'|' && bytes.get(i + 1) == Some(&b'|') {
                Some("||")
            } else {
                None
            }
        } else {
            None
        };
        if let Some(op) = operator {
            operands.push(std::mem::take(&mut current));
            operators.push(op);
            i += 2;
            continue;
        }
        match b {
            b'\'' => in_quote = !in_quote,
            b'(' if !in_quote => depth += 1,
            b')' if !in_quote => depth = depth.saturating_sub(1),
            _ => {}
        }
        // The target is ASCII (a `github` context expression over JSON triples), so each byte is
        // one code point; rebuilding it avoids `&expr[a..b]`, which `-D clippy::string-slice`
        // refuses even on boundaries.
        current.push(b as char);
        i += 1;
    }
    operands.push(current);
    SplitParts { operands, operators }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// The predicate, and the aarch64 pair / all-four set spelled as the workflow does inside
    /// `fromJSON`. Each refusal fixture builds one `target:` out of these.
    const A: &str = "github.event_name == 'pull_request'";
    const PAIR: &str = "[\"aarch64-unknown-linux-gnu\",\"aarch64-unknown-linux-musl\"]";
    const ALL: &str = "[\"x86_64-unknown-linux-gnu\",\"aarch64-unknown-linux-gnu\",\"x86_64-unknown-linux-musl\",\"aarch64-unknown-linux-musl\"]";

    /// A synthetic root carrying a working `cross-link.yml`, so a test can break exactly one input.
    ///
    /// Named per test rather than shared: the workflow file is written into, and two tests sharing
    /// one would pass or fail depending on which ran first.
    fn sound_root(tag: &str, target: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("sutura-cross-{}-{tag}", std::process::id()));
        if root.exists() {
            std::fs::remove_dir_all(&root).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(root.join(super::WORKFLOWS)).expect("temp workflows dir");
        std::fs::write(
            root.join(super::WORKFLOWS).join(super::FILE),
            format!("jobs:\n  link:\n    strategy:\n      matrix:\n        target: {target}\n"),
        )
        .expect("write cross-link.yml");
        root
    }

    /// Swap the two fromJSON bodies of `GOOD` (used by tests that need exactly that mutation).
    fn inverted() -> String {
        wrap(&format!("{A} && fromJSON('{ALL}') || fromJSON('{PAIR}')"))
    }

    fn drop_root(root: &PathBuf) {
        std::fs::remove_dir_all(root).expect("the temp tree this test created is removable");
    }

    /// Wrap a bare inner expression in the literal `${{ ... }}` a workflow requires. Kept as a
    /// helper because spelling that open brace pair through `format!` needs double escapes; the
    /// inner expression is passed already-formed.
    fn wrap(inner: &str) -> String {
        format!("${{{{ {inner} }}}}")
    }

    /// THE PRODUCTION ENTRY POINT, against the real tree. Every refusal below breaks one input to
    /// this same call, so `&& false` on any of them reddens one of the tests below rather than
    /// none - which is the difference between testing the predicate and testing the refusal.
    #[test]
    fn the_production_tree_holds_the_2_4_split() {
        let root = crate::repo::root().expect("repo root");
        let found = super::problems(&root);
        assert!(
            found.is_empty(),
            "the production cross matrix no longer holds the 2/4 split: {found:?}"
        );
    }

    /// The primary refusal: the pre-#477 shape, a plain four-list with no event selection. One
    /// event would build all four.
    #[test]
    fn a_plain_four_list_is_refused() {
        let root = sound_root("plain-list", "");
        let text = std::fs::read_to_string(root.join(super::WORKFLOWS).join(super::FILE)).unwrap();
        let text = text.replacen(
            "target: \n",
            "target:\n          - x86_64-unknown-linux-gnu\n          - aarch64-unknown-linux-gnu\n          - x86_64-unknown-linux-musl\n          - aarch64-unknown-linux-musl\n",
            1,
        );
        std::fs::write(root.join(super::WORKFLOWS).join(super::FILE), text).unwrap();
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("plain scalar list"), "{found:?}");
        drop_root(&root);
    }

    /// Correct three operands and correct sets, but the operators are the wrong way round
    /// (`A || B && C`): a structurally-shaped ternary that still runs the wrong set.
    #[test]
    fn a_correct_shaped_ternary_with_swapped_operators_is_refused() {
        let target = wrap(&format!("{A} || fromJSON('{PAIR}') && fromJSON('{ALL}')"));
        let root = sound_root("wrong-ops", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`&&` then `||`"), "{found:?}");
        drop_root(&root);
    }

    /// The ternary inverted so a PR gets all four and `main` gets two - the M1 regression.
    #[test]
    fn an_inverted_ternary_is_refused() {
        let root = sound_root("inverted", &inverted());
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("aarch64 pair"), "{found:?}");
        drop_root(&root);
    }

    /// A third PR leg defeats "two aarch64 on a branch" by building more surface per PR.
    #[test]
    fn a_third_pull_request_leg_is_refused() {
        let target = wrap(&format!(
            "{A} && fromJSON('[\"aarch64-unknown-linux-gnu\",\"aarch64-unknown-linux-musl\",\"x86_64-unknown-linux-gnu\"]') || fromJSON('{ALL}')"
        ));
        let root = sound_root("third-leg", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("aarch64 pair"), "{found:?}");
        drop_root(&root);
    }

    /// The pair is pinned order-sensitively, so swapping its two triples is a change, not a no-op.
    #[test]
    fn a_reordered_pull_request_pair_is_refused() {
        let target = wrap(&format!(
            "{A} && fromJSON('[\"aarch64-unknown-linux-musl\",\"aarch64-unknown-linux-gnu\"]') || fromJSON('{ALL}')"
        ));
        let root = sound_root("reordered", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("aarch64 pair"), "{found:?}");
        drop_root(&root);
    }

    /// Dropping a leg from the all-four set shrinks what `main` primes into the binary cache.
    #[test]
    fn a_partial_all_four_set_is_refused() {
        let target = wrap(&format!(
            "{A} && fromJSON('{PAIR}') || fromJSON('[\"x86_64-unknown-linux-gnu\",\"aarch64-unknown-linux-gnu\",\"x86_64-unknown-linux-musl\"]')"
        ));
        let root = sound_root("partial-all", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("all-four"), "{found:?}");
        drop_root(&root);
    }

    /// Rewriting the predicate event swaps which set a pull request gets.
    #[test]
    fn an_event_swap_is_refused() {
        let target = wrap(&format!(
            "github.event_name == 'push' && fromJSON('{PAIR}') || fromJSON('{ALL}')"
        ));
        let root = sound_root("event-swap", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("predicate"), "{found:?}");
        drop_root(&root);
    }

    /// Inverting the comparison to `!=` makes every non-PR event the pair and a PR the four.
    #[test]
    fn a_predicate_inversion_is_refused() {
        let target = wrap(&format!(
            "github.event_name != 'pull_request' && fromJSON('{PAIR}') || fromJSON('{ALL}')"
        ));
        let root = sound_root("pred-inv", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("predicate"), "{found:?}");
        drop_root(&root);
    }

    /// Moving the predicate off the first operand breaks the `A && B || C` contract even with the
    /// right sets and operators in the right order.
    #[test]
    fn an_operand_reorder_is_refused() {
        let target = wrap(&format!("fromJSON('{ALL}') && {A} || fromJSON('{PAIR}')"));
        let root = sound_root("operand-order", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("predicate"), "{found:?}");
        drop_root(&root);
    }

    /// A two-operand expression has no event split at all - the shape collapses to one set.
    #[test]
    fn a_two_operand_expression_is_refused() {
        let target = wrap(&format!("{A} && fromJSON('{PAIR}')"));
        let root = sound_root("two-operand", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("three operands"), "{found:?}");
        drop_root(&root);
    }

    /// A four-operand expression is a ternary that has grown an event branch with a new set.
    #[test]
    fn a_four_operand_expression_is_refused() {
        let target = wrap(&format!(
            "{A} && fromJSON('{PAIR}') || fromJSON('{ALL}') || fromJSON('[\"aarch64-unknown-linux-musl\"]')"
        ));
        let root = sound_root("four-operand", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("three operands"), "{found:?}");
        drop_root(&root);
    }

    /// A leg spelled as a bare array literal instead of `fromJSON(...)` is out of position: it
    /// cannot be resolved the way the workflow resolves it.
    #[test]
    fn a_fromjson_out_of_position_is_refused() {
        let target = wrap(&format!("{A} && {PAIR} || fromJSON('{ALL}')"));
        let root = sound_root("fromjson-pos", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("fromJSON"), "{found:?}");
        drop_root(&root);
    }

    /// Broken JSON inside `fromJSON` means the set cannot be resolved at all.
    #[test]
    fn an_unparseable_fromjson_is_refused() {
        let target = wrap(&format!("{A} && fromJSON('[{PAIR}') || fromJSON('{ALL}')"));
        let root = sound_root("unparseable", &target);
        let found = super::problems(&root);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("unparseable JSON"), "{found:?}");
        drop_root(&root);
    }
}
