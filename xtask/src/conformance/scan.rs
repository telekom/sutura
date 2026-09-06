//! Reading the two declarations this gate compares, out of Rust, as text.
//!
//! Both sides are macro syntax, which is why they are read here rather than derived from a
//! compiled artefact: the registry is a `macro_rules!` arm and a binding is an invocation of
//! another one, so neither exists as a value any tool can be asked for. `cargo metadata` is out
//! for a second reason - it needs a resolvable registry, and this gate runs inside the hygiene
//! sweep, in a nix sandbox with no network.
//!
//! **A gate that scans a language must lex it**, and the record in `.agents/skills/sutura/gates`
//! has three instances of a scan counting where it should have lexed - `check-workflows`' braces,
//! `check-docs`' fences, and the causality gate reading an attribute as one line. Every function
//! here therefore works over [`code_lines`](crate::serde_parse::scan::code_lines), which blanks
//! comments and multi-line string interiors, and **an unclosed delimiter is an `Err` rather than
//! an answer.** The blanking is load-bearing rather than tidy: this repository's prose writes
//! `execute_packs!` and `$cell!(..)` repeatedly while explaining the rule, and the packs crate's
//! own module header carries a whole worked example of a binding in a doc comment.
//!
//! **The limit both scans share, and it is the one `code_lines` declares:** a SINGLE-line string
//! literal keeps its interior, so a brace or a needle inside one is live. For [`module_paths`]
//! that means a doubled `{{` in a one-line literal makes the file unbalanced, which comes out as
//! an error naming the line rather than as a wrong module path. There is no such literal in any
//! file this gate lexes today, checked over every binding in the tree.

use crate::serde_parse::scan::code_lines;

/// The registry arm this gate reads, as it is written in the matcher.
///
/// Whitespace-dense, because every comparison here is against a line with its whitespace removed -
/// so the arm may be reformatted without moving the gate. The arm rather than the macro's name:
/// `docs/adr/0012` calls this the `data_systems` registry, and it is the arm - not the macro - that
/// decides which list is the matrix over data systems.
pub(crate) const REGISTRY_ARM: &str = "(data_systems:$cell:ident)";

/// The macro whose definition identifies the harness crate.
///
/// Discovered rather than named, for the reason `check-bounded-wait` discovers its waiter and
/// `check-examples` derives its own crate: a path constant is held by recall and fails OPEN when
/// it stops matching. The crate that DEFINES the packs is the crate whose own bindings are fakes,
/// and that is a property of where this macro is written rather than of a name.
pub(crate) const PACKS_MACRO: &str = "macro_rules!execute_packs";

/// A binding: an invocation of the packs macro.
pub(crate) const BINDING: &str = "execute_packs!";

/// One `$cell!(name, Adapter)` entry of a registry arm, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    /// The cell's name. **The key**: every consumer of this registry - a snapshot, a test module,
    /// the CI matrix `docs/adr/0012` says is emitted from it - spells this and nothing else.
    pub(crate) name: String,
    /// The adapter type, as written. Its first path segment is the crate the entry derives.
    pub(crate) adapter: String,
}

/// One invocation of the packs macro, and where it sits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invocation {
    /// 1-based line of the invocation.
    pub(crate) line: usize,
    /// The `adapter:` ident, which is the module the expansion puts its tests in.
    pub(crate) adapter: String,
    /// The module path the invocation sits inside, outermost first.
    ///
    /// Carried because it is half of the selector: the emitted test name is
    /// `<module path>::<adapter>::<behaviour>`, so a `-E 'test(conformance::duckdb)'` tier is only
    /// selectable while this is `["conformance"]`. `telekom/sutura#135` is the consumer.
    pub(crate) module: Vec<String>,
}

impl Invocation {
    /// The filter a reader spells to select this adapter's tier, derived from where it sits.
    pub(crate) fn selector(&self) -> String {
        let mut out = self.module.join("::");
        if !out.is_empty() {
            out.push_str("::");
        }
        out.push_str(&self.adapter);
        out
    }
}

/// A file's blanked lines, and the same lines with their whitespace removed.
pub(crate) type Lexed = (Vec<String>, Vec<String>);

/// The module path each line of a file sits inside, one entry per line.
pub(crate) type ModulePaths = Vec<Vec<String>>;

/// Every line of `text` with the non-code half blanked, plus each line's whitespace-dense form.
///
/// The two are returned together because every scan below wants both and computing the dense form
/// twice is how a needle comes to be matched against one and reported against the other.
pub(crate) fn lexed(text: &str) -> Lexed {
    let code = code_lines(text);
    let dense: Vec<String> = code
        .iter()
        .map(|line| line.chars().filter(|character| !character.is_whitespace()).collect())
        .collect();
    (code, dense)
}

/// The 1-based lines of `dense` containing `needle`.
pub(crate) fn sites(dense: &[String], needle: &str) -> Vec<usize> {
    dense
        .iter()
        .enumerate()
        .filter(|&(_, line)| line.contains(needle))
        .map(|(index, _)| index.saturating_add(1))
        .collect()
}

/// Every `$cell!(..)` entry of the arm opening at 1-based `line`.
///
/// The arm's body is joined into one dense string before it is parsed, so a cell wrapped across
/// lines by the formatter reads the same as a cell on one - which the `catalogs` arm next door
/// already is, and this arm becomes the day an adapter type gets long enough.
pub(crate) fn cells(dense: &[String], line: usize) -> Result<Vec<Cell>, String> {
    let body = block(dense, line, REGISTRY_ARM)?;
    let mut found = Vec::new();
    let mut rest = body.as_str();
    while let Some(at) = rest.find("$cell!(") {
        let after = rest.get(at.saturating_add("$cell!(".len())..).unwrap_or_default();
        let (group, tail) = group(after).ok_or_else(|| format!("line {line}: a `$cell!(` never closes"))?;
        let args = arguments(group);
        let [name, adapter] = args.as_slice() else {
            return Err(format!(
                "line {line}: `$cell!({group})` has {} argument(s) and a data-system cell has two - \
                 the name and the adapter type",
                args.len()
            ));
        };
        found.push(Cell {
            name: name.clone(),
            adapter: adapter.clone(),
        });
        rest = tail;
    }
    if found.is_empty() {
        return Err(format!("line {line}: the `data_systems` arm declares no `$cell!(..)` entry"));
    }
    Ok(found)
}

/// The invocation opening at 1-based `line`, with its `adapter:` and its module path.
///
/// `paths` is [`module_paths`]' answer for the same file, passed in rather than recomputed: one
/// lex per file, and a caller cannot pair a site with another file's paths.
pub(crate) fn invocation(dense: &[String], paths: &[Vec<String>], line: usize) -> Result<Invocation, String> {
    let body = block(dense, line, BINDING)?;
    let named: Vec<&str> = body
        .split("adapter:")
        .skip(1)
        .filter_map(|rest| rest.split(',').next())
        .collect();
    let [adapter] = named.as_slice() else {
        return Err(format!(
            "line {line}: this binding names {} `adapter:` argument(s), and the macro takes exactly one - \
             a binding whose adapter cannot be named is a binding this gate cannot reconcile",
            named.len()
        ));
    };
    if adapter.is_empty() {
        return Err(format!("line {line}: this binding's `adapter:` argument is empty"));
    }
    let module = paths
        .get(line.saturating_sub(1))
        .ok_or_else(|| format!("line {line}: is past the end of the file"))?
        .clone();
    Ok(Invocation {
        line,
        adapter: (*adapter).to_owned(),
        module,
    })
}

/// The module path every line sits inside, one entry per line, outermost first.
///
/// A brace walk over the blanked code, tracking whether each `{` was a module's - so the answer is
/// the path the compiler would give and not a guess from indentation. **An unbalanced file is an
/// error**: a `}` closing nothing, or a `{` still open at the end, is reported with its line rather
/// than yielding a path a caller cannot tell from a correct one.
///
/// A line's own path is recorded BEFORE its braces are walked, which is what makes `mod x {` sit
/// outside `x` and everything below it inside.
pub(crate) fn module_paths(code: &[String]) -> Result<ModulePaths, String> {
    let mut out: ModulePaths = Vec::with_capacity(code.len());
    let mut open: Vec<Option<String>> = Vec::new();
    for (index, line) in code.iter().enumerate() {
        out.push(open.iter().flatten().cloned().collect());
        walk(line, &mut open).map_err(|why| format!("line {}: {why}", index.saturating_add(1)))?;
    }
    if !open.is_empty() {
        return Err(format!("{} block(s) are still open at the end of the file", open.len()));
    }
    Ok(out)
}

/// One line's braces, applied to the stack of open blocks.
///
/// A `{` preceded by the two words `mod <ident>` opens a named block; every other `{` opens an
/// anonymous one, which is tracked too - the path is only right while every brace is accounted for.
fn walk(line: &str, open: &mut Vec<Option<String>>) -> Result<(), String> {
    let mut before = String::new();
    let mut previous = String::new();
    let mut word = String::new();
    for character in line.chars() {
        if character.is_alphanumeric() || character == '_' {
            word.push(character);
            continue;
        }
        if !word.is_empty() {
            before = core::mem::replace(&mut previous, core::mem::take(&mut word));
        }
        match character {
            '{' => open.push((before == "mod").then(|| previous.clone())),
            '}' => drop(
                open.pop()
                    .ok_or_else(|| String::from("a `}` closes a block that was never opened"))?,
            ),
            _ => {}
        }
    }
    Ok(())
}

/// The delimited body that opens after `needle` on 1-based `line` of `dense`, without delimiters.
///
/// **The scan begins after the needle rather than at the start of the line, and that is not
/// cosmetic:** the registry arm's line is `(data_systems: $cell:ident) => {`, so the first opening
/// delimiter on it belongs to the macro's MATCHER - a `block` reading from the start of the line
/// would return `data_systems:$cell:ident` and the caller would find no cells in a registry that
/// has three.
///
/// Whatever mixture of `(`, `[` and `{` lies between counts, because a macro invocation is written
/// with any of the three and these two declarations use two of them. Refuses a needle that opens
/// no block, and a delimiter that never closes.
fn block(dense: &[String], line: usize, needle: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut depth = 0_usize;
    let mut started = false;
    let mut reached = false;
    for text in dense.iter().skip(line.saturating_sub(1)) {
        let from = if reached {
            text.as_str()
        } else {
            let Some(at) = text.find(needle) else {
                continue;
            };
            reached = true;
            text.get(at.saturating_add(needle.len())..).unwrap_or_default()
        };
        for character in from.chars() {
            match character {
                '(' | '[' | '{' => {
                    if started {
                        out.push(character);
                    }
                    depth = depth.saturating_add(1);
                    started = true;
                }
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && started {
                        return Ok(out);
                    }
                    out.push(character);
                }
                _ if started => out.push(character),
                _ => {}
            }
        }
    }
    if started {
        return Err(format!("line {line}: a delimiter opens here and never closes"));
    }
    Err(format!("line {line}: `{needle}` opens no delimited block here"))
}

/// The balanced parenthesised group at the start of `text`, and what follows it.
fn group(text: &str) -> Option<(&str, &str)> {
    let mut depth = 1_usize;
    for (at, character) in text.char_indices() {
        match character {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((text.get(..at)?, text.get(at.saturating_add(1)..)?));
                }
            }
            _ => {}
        }
    }
    None
}

/// A macro group's arguments, split at the commas that are not inside a nested group.
///
/// `<` and `>` count, because an adapter type may be generic - the `catalogs` arm's second entry
/// is - and a comma inside its type arguments is not an argument of the cell.
fn arguments(group: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0_usize;
    let mut current = String::new();
    for character in group.chars() {
        match character {
            '(' | '[' | '{' | '<' => {
                depth = depth.saturating_add(1);
                current.push(character);
            }
            ')' | ']' | '}' | '>' => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            ',' if depth == 0 => out.push(core::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Cell, cells, invocation, lexed, module_paths, sites};

    /// The registry arm as it is written, indented inside the macro it belongs to.
    const ARM: &str = "
macro_rules! registered {
    (data_systems: $cell:ident) => {
        // A comment naming $cell!(commented, sutura_exec_ghost::Ghost) to be blanked.
        $cell!(datafusion, sutura_exec_datafusion::DataFusionWarehouse);
        $cell!(duckdb, sutura_exec_duckdb::DuckDbWarehouse);
    };
}
";

    fn arm_cells(text: &str) -> Result<Vec<Cell>, String> {
        let (_, dense) = lexed(text);
        let line = *sites(&dense, super::REGISTRY_ARM).first().expect("the fixture has the arm");
        cells(&dense, line)
    }

    #[test]
    fn the_registry_arms_cells_are_read_with_their_adapter_types() {
        let found = arm_cells(ARM).expect("the fixture arm parses");
        assert_eq!(
            found,
            vec![
                Cell {
                    name: String::from("datafusion"),
                    adapter: String::from("sutura_exec_datafusion::DataFusionWarehouse"),
                },
                Cell {
                    name: String::from("duckdb"),
                    adapter: String::from("sutura_exec_duckdb::DuckDbWarehouse"),
                },
            ]
        );
    }

    /// A cell in a comment is not a registration, and the reason this is asserted rather than
    /// assumed is that the file next door explains the registry in prose that names cells.
    #[test]
    fn a_cell_inside_a_comment_is_not_read_as_a_registration() {
        let found = arm_cells(ARM).expect("the fixture arm parses");
        assert!(!found.iter().any(|cell| cell.name == "commented"), "{found:?}");
    }

    /// The formatter wraps a long cell, and a wrapped one is the same registration.
    #[test]
    fn a_cell_wrapped_across_lines_is_one_registration() {
        let text = "
    (data_systems: $cell:ident) => {
        $cell!(
            datahub,
            sutura_catalog_datahub::DataHubCatalog<sutura_catalog_datahub::fixture::FixtureReader>
        );
    };
";
        let found = arm_cells(text).expect("a wrapped cell parses");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found.first().map(|cell| cell.name.as_str()), Some("datahub"));
    }

    /// A cell whose shape this gate cannot read is an ERROR and not an entry it skips: a
    /// registration the scan cannot see is exactly the hole the gate exists to close.
    #[test]
    fn a_cell_with_the_wrong_number_of_arguments_is_an_error() {
        let text = "    (data_systems: $cell:ident) => {\n        $cell!(lonely);\n    };\n";
        let why = arm_cells(text).expect_err("a one-argument cell is not a data-system cell");
        assert!(why.contains("has 1 argument(s)"), "{why}");
    }

    /// An arm with no cell at all is an error, not an empty registry: an empty scan is how this
    /// property went unheld in the first place.
    #[test]
    fn an_arm_with_no_cells_is_an_error() {
        let text = "    (data_systems: $cell:ident) => {\n    };\n";
        let why = arm_cells(text).expect_err("an empty arm is an error");
        assert!(why.contains("declares no `$cell!(..)` entry"), "{why}");
    }

    /// A binding as the two adapters in this tree write it, inside the wrapper module.
    const BOUND: &str = r#"
#[cfg(test)]
mod conformance {
    fn open() -> DuckDbWarehouse {
        DuckDbWarehouse::in_memory(corpus::source(), corpus::posture()).expect("it opens")
    }

    sutura_conformance::execute_packs! {
        adapter: duckdb,
        warehouse: sutura_exec_duckdb::DuckDbWarehouse,
        open: crate::conformance::open,
        executes_legs,
    }
}
"#;

    fn bound(text: &str) -> Result<super::Invocation, String> {
        let (code, dense) = lexed(text);
        let paths = module_paths(&code)?;
        let line = *sites(&dense, super::BINDING).first().expect("the fixture has a binding");
        invocation(&dense, &paths, line)
    }

    #[test]
    fn a_binding_is_read_with_its_adapter_and_the_module_it_sits_in() {
        let found = bound(BOUND).expect("the fixture binding parses");
        assert_eq!(found.adapter, "duckdb");
        assert_eq!(found.module, vec![String::from("conformance")]);
        // The selector is DERIVED from the two, which is what makes it checkable at all.
        assert_eq!(found.selector(), "conformance::duckdb");
    }

    #[test]
    fn a_binding_outside_the_wrapper_module_has_no_module_in_its_selector() {
        let text = "sutura_conformance::execute_packs! {\n    adapter: duckdb,\n    executes_legs,\n}\n";
        let found = bound(text).expect("a bare binding parses");
        assert!(found.module.is_empty(), "{:?}", found.module);
        assert_eq!(found.selector(), "duckdb");
    }

    /// The same refusal shape the causality gate had to learn: a declaration whose subject cannot
    /// be NAMED is a refusal, because every comparison below it would be about nothing.
    #[test]
    fn a_binding_that_names_no_adapter_is_an_error() {
        let text = "sutura_conformance::execute_packs! {\n    warehouse: X,\n    executes_legs,\n}\n";
        let why = bound(text).expect_err("a binding with no adapter is an error");
        assert!(why.contains("names 0 `adapter:` argument(s)"), "{why}");
    }

    #[test]
    fn a_module_path_is_nested_and_a_function_body_is_not_a_module() {
        let text = "mod outer {\n    mod inner {\n        fn f() {\n            let x = 1;\n        }\n    }\n}\n";
        let paths = module_paths(&super::lexed(text).0).expect("the fixture is balanced");
        let inside_the_function = paths.get(3).expect("line 4 exists");
        assert_eq!(inside_the_function, &vec![String::from("outer"), String::from("inner")]);
    }

    /// An unbalanced file is an ERROR, and this repository has three recorded instances of a scan
    /// INVENTING an answer where it should have refused.
    #[test]
    fn a_brace_that_closes_nothing_is_an_error() {
        let why = module_paths(&super::lexed("fn f() {\n}\n}\n").0).expect_err("an extra brace is an error");
        assert!(why.contains("line 3") && why.contains("never opened"), "{why}");
    }

    #[test]
    fn a_block_left_open_is_an_error() {
        let why = module_paths(&super::lexed("mod m {\n").0).expect_err("an open block is an error");
        assert!(why.contains("still open"), "{why}");
    }

    /// **The declared limit, asserted so it stays declared.** `code_lines` keeps a ONE-LINE
    /// string's interior, so a doubled brace in one is counted - and the honest consequence is an
    /// error naming a line, never a module path a caller cannot tell from a correct one. No file
    /// this gate lexes carries such a literal today.
    #[test]
    fn a_doubled_brace_in_a_one_line_literal_is_an_error_rather_than_a_wrong_answer() {
        let text = "mod m {\n    fn f() { println!(\"{{\"); }\n}\n";
        let why = module_paths(&super::lexed(text).0).expect_err("the literal's brace is counted");
        assert!(why.contains("still open"), "{why}");
    }
}
