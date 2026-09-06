//! Every shell body a devenv module hands to bash goes through the wrapper `shellcheck` reads.
//!
//! `devenv.nix`'s `linted` wraps a body in `pkgs.writeShellApplication`, whose `checkPhase` is
//! `bash -n` plus `shellcheck` - so a body that does not pass cannot be BUILT, and a script that
//! cannot be built cannot be run. That is the linter. This is the gate that keeps every body
//! inside it, and it exists because the sentence claiming so was false.
//!
//! # What the needle it replaces could not say
//!
//! The first version of this rule was two forbidden literals, `.exec = "` and `.exec = ''`, over
//! the single path `devenv.nix`. `github.com/telekom/sutura#402` walked past it **six ways**, each
//! measured on the merged tree with a body carrying three `shellcheck` findings and every gate
//! green: no leading dot (`{ exec = "..." }`), two spaces before the quote, a newline after the
//! `=`, the `''...''` form, **any attribute that is not `exec`** - `enterTest`, and `enterShell`,
//! which is fifty-seven lines running on every shell entry - and **the same literal in a module
//! `imports` reaches**, because a slash-less pattern matches a bare basename and nothing else.
//!
//! All six are one assignment to a reader that keys on the `=` and on the ATTRIBUTE NAME, which
//! is what this does. See [`scan`] for why the code projection is enough to tell a wrapped body
//! from a bare one, and [`tally`] for why the number in the verdict is read off the set that was
//! judged rather than printed beside it.
//!
//! # Three rules, and the third one is about the emission rather than the spelling
//!
//! 1. **Every shell-bearing assignment routes through a wrapper.** The wrapper set is DERIVED,
//!    not listed: a `let` binding whose value reaches `writeShellApplication` is one,
//!    transitively, so `runs`, `onStable` and `sourced` are wrappers because they reach `linted`.
//!    A binding that only looks like one is not.
//! 2. **The scan is fail-closed at both ends.** An unreadable module, an `imports` entry this
//!    cannot follow, no wrapper at all, a wrapper NAME bound twice, and a discovery pass that
//!    found NOTHING are each a refusal. #402's own measurement of this repository's gates is the
//!    argument: over a whole-repo falsifier tree only 3 of 31 refused because their own rule
//!    fired, 20 refused on a missing input and 8 on an empty-scan floor - so a gate wants all
//!    three arms, and wants to say which one answered.
//! 3. **The wrapper's argument set is CLOSED.** #402's seventh escape defeated the linter rather
//!    than the rule: `checkPhase = "true";` added to `linted` removes `bash -n` AND `shellcheck`
//!    from every body, and every textual gate stayed green because they held the spelling
//!    `onStable`/`runs` rather than the emission. A blocklist of that one attribute would not
//!    have closed it either: `doCheck`, `checkInputs` and `derivationArgs` each do the same job.
//!    So what is held is that `writeShellApplication` is handed nothing but the three arguments a
//!    body needs. See [`WRAPPER_ARGUMENTS`].
//!
//! # The limits, next to the claims
//!
//! * **This gate is structural. It does not run `shellcheck`**, and cannot: the sweep it is in
//!   runs inside a nix derivation with no nix. The EMISSION is read by
//!   `cargo xtask check-devenv-linter`, which needs a store the wrapper has been built into -
//!   `just devenv-linter` is that venue, and `just ship-check` runs it when a diff touches a
//!   devenv module. So on a pull request the structural half runs and the linter half does not.
//! * **A one-line string body under an attribute name this does not know is unheld.** Rule 1 keys
//!   on [`SHELL_ATTRIBUTES`], which is devenv's shell options and not a list nix can derive here;
//!   rule 1b catches a multi-line literal whatever it is assigned to, because a shell body of any
//!   size is a block. What falls between is a single-line body under a NEW devenv option.
//! * **A module devenv loads and git does not publish is invisible.** `devenv.local.nix` is
//!   gitignored by design, so it is neither read nor held.
//! * **`-x` is not passed, and the tracked `*.sh` files get it.** `nix/lint-workflows.sh` runs
//!   `nix run .#shellcheck -- -x`; `writeShellApplication`'s checkPhase does not, and adding it
//!   would mean overriding `checkPhase` - which is exactly what rule 3 refuses. The one `source`
//!   in these bodies is a store path already marked `source=/dev/null`, so `-x` would follow
//!   nothing today. Stated rather than closed, because closing it reopens the seventh escape.

use std::collections::BTreeSet;
use std::path::Path;

use crate::Verdict;
use crate::repo;
use scan::{Assignment, Value};
use tally::{Body, Discovered, Held, Judged};

mod scan;
mod tally;

/// The devenv module every other one is reached from.
const ROOT_MODULE: &str = "devenv.nix";

/// The devenv options whose value is a shell body.
///
/// Written down because nothing here can derive it - devenv's option set lives in its own flake,
/// and this gate runs where there is no nix. `exec` covers `scripts`, `tasks` and `processes`
/// alike, which is the whole reason the attribute NAME is the key: the path in front of it is
/// what the six measured spellings differed in.
const SHELL_ATTRIBUTES: &[&str] = &["enterShell", "enterTest", "exec", "startupCommand"];

/// The only attributes the wrapper may hand `writeShellApplication`.
///
/// A CLOSED set, and that is the whole of rule 3. Every way to remove the linter is an attribute
/// passed through to the derivation: `checkPhase` replaces the phase, `doCheck` skips it,
/// `checkInputs` empties its tool set, `derivationArgs` carries any of those one level down. A
/// blocklist would be a list of spellings to keep complete; this is the complement, so a
/// wrapper that grows an argument is a visible diff a reviewer has to agree with.
const WRAPPER_ARGUMENTS: &[&str] = &["name", "bashOptions", "text"];

/// The nixpkgs builder whose `checkPhase` is the linter. The wrapper set is seeded from it.
const LINTER: &str = "writeShellApplication";

/// One devenv module, read.
struct Module {
    /// Repo-relative path, for a message a reader can open.
    rel: String,
    /// Every assignment in it.
    assignments: Vec<Assignment>,
}

/// Read `devenv.nix` and every module its `imports` reach.
///
/// FAILS CLOSED on anything it cannot read, and that is deliberate rather than defensive: #402's
/// sixth escape was the identical literal in an imported module, which the old rule READ and
/// declined to judge. A module this cannot follow is the same hole with a different cause, so it
/// is a refusal naming the entry rather than a silent narrowing.
fn modules(root: &Path) -> Result<Vec<Module>, String> {
    let mut queue = vec![String::from(ROOT_MODULE)];
    let mut seen = BTreeSet::new();
    let mut read = Vec::new();

    while let Some(rel) = queue.pop() {
        if !seen.insert(rel.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&rel))
            .map_err(|error| format!("could not read the devenv module {rel}: {error}"))?;
        let assignments = scan::assignments(&text);
        for import in imports(&rel, &assignments)? {
            queue.push(import);
        }
        read.push(Module { rel, assignments });
    }
    Ok(read)
}

/// Is this token a Nix file? Through `Path::extension`, which is what the workspace's lint set
/// asks for in place of a suffix compare.
fn is_nix(token: &str) -> bool {
    Path::new(token)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nix"))
}

/// The modules one module's `imports` names, as repo-relative paths.
///
/// Only a relative path literal is followed, and anything else in the list is a refusal: an
/// import that resolves through a flake input is a module this gate cannot read, and a gate that
/// silently holds less than the tree is the defect it exists to close.
fn imports(rel: &str, assignments: &[Assignment]) -> Result<Vec<String>, String> {
    let directory = rel.rsplit_once('/').map_or("", |(head, _)| head);
    let mut found = Vec::new();
    for assignment in assignments.iter().filter(|a| a.lets == 0 && a.attribute == "imports") {
        for token in assignment.code.split_whitespace() {
            let token = token.trim_end_matches(']').trim_start_matches('[');
            if token.is_empty() {
                continue;
            }
            let named = token.strip_prefix("./").filter(|p| is_nix(p));
            let Some(path) = named else {
                return Err(format!(
                    "{rel}:{}: `imports` names `{token}`, which this gate cannot read as a file - \
                     so the shell bodies in it would be held by nothing. Import a relative `.nix` \
                     path, or state the module here",
                    assignment.line
                ));
            };
            found.push(if directory.is_empty() {
                String::from(path)
            } else {
                format!("{directory}/{path}")
            });
        }
    }
    Ok(found)
}

/// Which `let` bindings reach the linter, transitively.
///
/// DERIVED rather than named, which is what makes a new wrapper work the day it is written and a
/// binding that only looks like one still fail. Seeded from the bindings that mention
/// [`LINTER`] - so replacing `writeShellApplication` with a builder that has no `checkPhase`
/// empties this set, and the empty set is a refusal.
fn wrappers(modules: &[Module]) -> BTreeSet<String> {
    let bindings: Vec<&Assignment> = modules
        .iter()
        .flat_map(|module| module.assignments.iter())
        .filter(|a| a.lets > 0)
        .collect();

    let mut set: BTreeSet<String> = bindings
        .iter()
        .filter(|a| scan::mentions(&a.code, LINTER))
        .map(|a| a.attribute.clone())
        .collect();

    loop {
        let grown: BTreeSet<String> = bindings
            .iter()
            .filter(|a| {
                !set.contains(&a.attribute)
                    && set
                        .iter()
                        .any(|known| known != &a.attribute && scan::mentions(&a.code, known))
            })
            .map(|a| a.attribute.clone())
            .collect();
        if grown.is_empty() {
            return set;
        }
        set.extend(grown);
    }
}

/// A wrapper name bound more than once, if there is one.
///
/// **A NAME is the whole of rule 1, so a name that means two things defeats it.** Bind
/// `onStable = name: body: body;` in a nested `let` and a body assigned through it reads as
/// routed while reaching no linter - the text scan cannot say which binding a use site resolves
/// to, and Nix scoping is not modelled here. So a duplicate is refused rather than resolved:
/// this is a refusal over a shape nobody writes, which is what a ratchet on a hole should be.
fn shadowed(modules: &[Module], wrapped: &BTreeSet<String>) -> Option<String> {
    for name in wrapped {
        let sites: Vec<String> = modules
            .iter()
            .flat_map(|module| {
                module
                    .assignments
                    .iter()
                    .filter(|a| a.lets > 0 && &a.attribute == name)
                    .map(|a| format!("{}:{}", module.rel, a.line))
            })
            .collect();
        if sites.len() > 1 {
            return Some(format!(
                "`{name}` is bound {} times ({}) - a body assigned through that name reaches \
                 whichever binding Nix scoping picks, and this gate reads text rather than \
                 evaluating it. One binding per wrapper name, or the routing rule means nothing",
                sites.len(),
                sites.join(", ")
            ));
        }
    }
    None
}

/// The argument set the wrapper hands the linter, or why it could not be read.
///
/// Returns the set so the verdict can name it: a reader who sees `name, bashOptions, text` knows
/// the checkPhase is nixpkgs' own, which is the only argument this gate can make without a store
/// to read a derivation out of.
fn arguments(modules: &[Module]) -> Result<BTreeSet<String>, String> {
    let calls: Vec<&Assignment> = modules
        .iter()
        .flat_map(|module| module.assignments.iter())
        .filter(|a| scan::mentions(&a.code, LINTER))
        .collect();

    let call = match calls.as_slice() {
        [] => {
            return Err(format!(
                "no devenv module applies `{LINTER}` - the wrapper whose checkPhase IS ShellCheck \
                 is gone, so nothing reads any script body. `pkgs.writeShellScriptBin` and \
                 `pkgs.writeShellScript` both build a body nothing has read"
            ));
        }
        [only] => only,
        many => {
            return Err(format!(
                "{} devenv bindings apply `{LINTER}` - two wrappers are two argument sets to keep \
                 in step, and a body goes through one of them",
                many.len()
            ));
        }
    };

    let mut names = BTreeSet::new();
    for member in members(&call.code)? {
        let member = member.trim();
        if member.is_empty() {
            continue;
        }
        if let Some(inherited) = member.strip_prefix("inherit ") {
            names.extend(inherited.split_whitespace().map(String::from));
        } else if let Some((key, _)) = member.split_once('=') {
            names.insert(String::from(key.trim()));
        } else {
            return Err(format!(
                "cannot read `{member}` as an argument to `{LINTER}` - this gate holds that set \
                 CLOSED, so an argument it cannot name is refused rather than passed over"
            ));
        }
    }
    Ok(names)
}

/// The `;`-separated members of the first attrset in `code`.
fn members(code: &str) -> Result<Vec<String>, String> {
    let mut depth = 0_i32;
    let mut inside = String::new();
    let mut opened = false;
    for c in code.chars() {
        match c {
            '{' => {
                depth = depth.saturating_add(1);
                opened = true;
                continue;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if opened && depth == 0 {
                    return Ok(inside.split(';').map(String::from).collect());
                }
                continue;
            }
            _ => {}
        }
        if opened && depth == 1 {
            inside.push(c);
        }
    }
    Err(format!(
        "the `{LINTER}` application has no attrset this gate can read: `{}`",
        code.trim()
    ))
}

/// Every shell-bearing assignment in the module set.
///
/// Two reasons an assignment is picked up, and they are stated on the body because their limits
/// differ: an attribute devenv treats as shell, or a multi-line string literal - which is what a
/// shell body of any size looks like, whatever it is assigned to.
fn discover(modules: &[Module]) -> Discovered {
    let mut bodies = Vec::new();
    for module in modules {
        for assignment in assignments_of(module) {
            let because = if SHELL_ATTRIBUTES.contains(&assignment.attribute.as_str()) {
                "a devenv option whose value is a shell body"
            } else if matches!(assignment.value, Value::Literal { lines } if lines > 1) {
                "a multi-line string literal, which is the shape of a shell body"
            } else {
                continue;
            };
            bodies.push(Body {
                module: module.rel.clone(),
                line: assignment.line,
                path: assignment.path.clone(),
                attribute: assignment.attribute.clone(),
                value: assignment.value.clone(),
                because,
            });
        }
    }
    Discovered::of(bodies)
}

/// A module's assignments at module level - not the `let` bindings above them.
///
/// A named helper rather than the filter inline, because the `lets == 0` half is a claim: a
/// binding inside `let ... in` is not a devenv option, and at brace depth alone the two are
/// indistinguishable. `crate::workflows::code_lines` is what answers it.
fn assignments_of(module: &Module) -> impl Iterator<Item = &Assignment> {
    module.assignments.iter().filter(|a| a.lets == 0)
}

/// Did this body's value go through a wrapper?
fn judge(body: &Body, wrappers: &BTreeSet<String>) -> Held {
    match &body.value {
        Value::Head(head) if wrappers.contains(head) => Held::Routed(head.clone()),
        _ => Held::Loose,
    }
}

/// What a loose body's failure line says.
fn loose(body: &Body, wrappers: &BTreeSet<String>) -> String {
    let through = wrappers.iter().cloned().collect::<Vec<String>>().join("`, `");
    let shape = match &body.value {
        Value::Head(head) => format!("its value begins with `{head}`, which reaches no wrapper"),
        Value::Structure => String::from("its value is a set or a list, so no wrapper saw the body"),
        Value::Literal { lines } => {
            format!("its value is a bare string literal over {lines} line(s)")
        }
    };
    format!(
        "{}:{}: `{}` assigns the `{}` option, which is {} - {shape}.\n      \
         Assign it through one of `{through}`, whose checkPhase is `bash -n` plus ShellCheck. A \
         body that goes nowhere near them is shell nothing reads: the `shellcheck` hook is \
         `files: \\.sh$`, `nix/lint-workflows.sh` globs tracked `*.sh`, and a Nix string is \
         neither",
        body.module, body.line, body.path, body.attribute, body.because
    )
}

pub(crate) fn run(_args: &[String]) -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-devenv-shell: could not locate the repo root");
        return Verdict::Fail;
    };

    let read = match modules(&root) {
        Ok(read) => read,
        Err(reason) => return refuse("a module it could not read", &[reason]),
    };

    let wrapped = wrappers(&read);
    if wrapped.is_empty() {
        return refuse(
            "its own rule",
            &[format!(
                "no `let` binding in any devenv module reaches `{LINTER}` - so no body is read by \
                 ShellCheck, whatever it is assigned through"
            )],
        );
    }

    if let Some(reason) = shadowed(&read, &wrapped) {
        return refuse("its own rule", &[reason]);
    }

    let declared = match arguments(&read) {
        Ok(declared) => declared,
        Err(reason) => return refuse("its own rule", &[reason]),
    };
    let expected: BTreeSet<String> = WRAPPER_ARGUMENTS.iter().map(|a| String::from(*a)).collect();
    if declared != expected {
        return refuse(
            "its own rule",
            &[format!(
                "`{LINTER}` is handed {declared:?} and this gate holds it to {expected:?}. Any \
                 further attribute reaches the derivation: `checkPhase` replaces the phase, \
                 `doCheck` skips it, `checkInputs` empties its tool set, `derivationArgs` carries \
                 any of them one level down - and one line of that removes `bash -n` AND \
                 ShellCheck from every body with every textual gate still green"
            )],
        );
    }

    let found = discover(&read);
    if found.is_empty() {
        return refuse(
            "an empty-scan floor",
            &[format!(
                "found no shell body in {} devenv module(s) - the dev shell is built out of them, \
                 so an empty read is a broken scan and not a clean tree",
                read.len()
            )],
        );
    }

    let judged = Judged::of(found, |body| judge(body, &wrapped));
    let mut problems: Vec<String> = judged.gap().into_iter().collect();
    problems.extend(
        judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| loose(&row.body, &wrapped)),
    );

    if problems.is_empty() {
        // The wrappers actually USED, not the ones derived: a chain that has grown a link nothing
        // routes through is visible here rather than only in the source.
        let used: BTreeSet<&str> = judged
            .rows()
            .iter()
            .filter_map(|row| match &row.held {
                Held::Routed(through) => Some(through.as_str()),
                Held::Loose => None,
            })
            .collect();
        println!(
            "xtask check-devenv-shell: ok - {} in {} module(s), through `{}` of {} derived \
             wrapper(s) over `{LINTER}`({})",
            judged.sentence(),
            read.len(),
            used.into_iter().collect::<Vec<&str>>().join("`, `"),
            wrapped.len(),
            declared.iter().cloned().collect::<Vec<String>>().join(", ")
        );
        return Verdict::Pass;
    }

    refuse("its own rule", &problems)
}

/// One refusal shape, so every arm says WHICH arm answered.
///
/// #402's diagnostic is the reason this is a parameter rather than a sentence: a gate that
/// refuses on a missing input and a gate whose rule fired look identical from a green run, and
/// only 3 of this repository's 31 gates were measured refusing on their own rule. Printing the
/// arm makes the difference readable in the failure rather than only in the source.
fn refuse(arm: &str, problems: &[String]) -> Verdict {
    eprintln!("xtask check-devenv-shell: FAILED - refused by {arm}");
    for problem in problems {
        eprintln!("  {problem}");
    }
    eprintln!();
    eprintln!("A devenv script body is shell, and the only thing that reads it is `linted`'s");
    eprintln!("checkPhase in devenv.nix - `bash -n` plus ShellCheck, run when the shell is BUILT.");
    eprintln!("`just devenv-linter` reads that checkPhase out of the store, so what this gate");
    eprintln!("holds structurally is provable rather than asserted.");
    Verdict::Fail
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{Held, Judged, LINTER, Module, WRAPPER_ARGUMENTS, arguments, discover, imports, judge, scan, wrappers};

    /// A devenv module with the real shapes: the wrapper chain in a `let`, and wrapped bodies.
    const GOOD: &str = r#"{ pkgs, ... }:
let
  linted = name: bashOptions: text:
    pkgs.writeShellApplication { name = "sutura-${name}"; inherit bashOptions text; };
  runs = name: body: "${linted name [ "errexit" ] body}/bin/sutura-${name}";
  sourced = name: body: "source ${linted name [ ] body}/bin/sutura-${name}";
  onStable = name: body: runs name ''
    source ${./nix/stable-env.sh}
    ${body}
  '';
in
{
  env.PLAIN = "not shell";
  enterShell = sourced "enter-shell" ''
    echo hello
  '';
  scripts = {
    fmt.exec = onStable "fmt" ''
      cargo run -q -p xtask -- fmt
    '';
    secrets.exec = runs "secrets" "betterleaks dir .";
  };
}
"#;

    /// The one line in [`GOOD`] a planted body replaces.
    const ANCHOR: &str = "  env.PLAIN = \"not shell\";";

    fn module(text: &str) -> Vec<Module> {
        vec![Module {
            rel: String::from("devenv.nix"),
            assignments: scan::assignments(text),
        }]
    }

    /// Every body in `text` the gate would refuse, by assigned path.
    fn loose_paths(text: &str) -> Vec<String> {
        let read = module(text);
        let wrapped = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &wrapped));
        judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| row.body.path.clone())
            .collect()
    }

    #[test]
    fn the_wrapper_chain_is_derived_from_the_builder_and_not_listed() {
        let derived = wrappers(&module(GOOD));
        assert!(derived.contains("linted"), "the seed reaches the builder: {derived:?}");
        assert!(derived.contains("runs"), "one hop: {derived:?}");
        assert!(derived.contains("onStable"), "two hops: {derived:?}");
        assert!(derived.contains("sourced"), "one hop the other way: {derived:?}");
        // A binding that only looks like a wrapper is not one.
        let faked = wrappers(&module(
            "let\n  linted = n: b: t: pkgs.writeShellApplication { };\n  fake = n: b: b;\nin\n{ }",
        ));
        assert!(faked.contains("linted"));
        assert!(!faked.contains("fake"), "{faked:?}");
    }

    #[test]
    fn a_wrapper_name_bound_twice_is_a_refusal_rather_than_a_guess() {
        // A NAME is the whole of rule 1, so a second binding of one - `onStable = n: b: b;` in a
        // nested `let` - would make a routed body reach no linter. Nix scoping is not modelled
        // here, so the duplicate is refused instead of resolved.
        let read = module(&GOOD.replace("in\n{", "  nested = let onStable = n: b: b; in onStable;\nin\n{"));
        let wrapped = wrappers(&read);
        let refusal = super::shadowed(&read, &wrapped).expect("a shadowed wrapper refuses");
        assert!(refusal.contains("`onStable` is bound 2 times"), "{refusal}");
        // And the real tree has one binding per wrapper name.
        assert!(super::shadowed(&module(GOOD), &wrappers(&module(GOOD))).is_none());
    }

    #[test]
    fn a_builder_with_no_checkphase_empties_the_wrapper_set() {
        // The seed IS the builder's name, so swapping it for one that reads nothing empties the
        // set - which `run` refuses on, rather than following the rename.
        let swapped = wrappers(&module(
            "let\n  linted = n: t: pkgs.writeShellScriptBin n t;\nin\n{ enterTest = linted \"a\" \"b\"; }",
        ));
        assert!(swapped.is_empty(), "{swapped:?}");
    }

    #[test]
    fn every_measured_escape_is_a_loose_body() {
        // One row per spelling `github.com/telekom/sutura#402` measured GREEN on the merged tree,
        // plus one this gate adds: an option name it does not know, written as a block.
        let cases = [
            (
                "no leading dot",
                "  scripts = { a = { exec = \"for f in $(ls *.rs); do echo $f; done\"; }; };",
            ),
            ("two spaces", "  scripts.a.exec =  \"for f in $(ls *.rs); do echo $f; done\";"),
            (
                "newline after the =",
                "  scripts.a.exec =\n    \"for f in $(ls *.rs); do echo $f; done\";",
            ),
            (
                "the block form",
                "  scripts.a.exec = ''\n    for f in $(ls *.rs); do echo $f; done\n  '';",
            ),
            (
                "a non-exec option",
                "  enterTest = ''\n    for f in $(ls *.rs); do echo $f; done\n  '';",
            ),
            (
                "an option this gate does not know",
                "  novelOption = ''\n    for f in $(ls *.rs); do echo $f; done\n  '';",
            ),
        ];
        for (label, planted) in cases {
            let text = GOOD.replace(ANCHOR, planted);
            assert!(!loose_paths(&text).is_empty(), "{label}: escaped\n{text}");
        }
    }

    #[test]
    fn the_real_shapes_are_not_flagged() {
        assert_eq!(loose_paths(GOOD), Vec::<String>::new());
    }

    #[test]
    fn a_body_glued_into_a_string_around_a_wrapper_is_still_loose() {
        // `enterShell`'s shape before this gate: a `''source ${linted ...}...''` literal, so the
        // OUTER shell - the `source` line itself - went through no wrapper. One line today and a
        // reader would call it harmless, which is why the rule keys on the value and not the size.
        let text = GOOD.replace(
            "  enterShell = sourced \"enter-shell\" ''\n    echo hello\n  '';",
            "  enterShell = ''source ${linted \"enter-shell\" [ ] ''echo hello''}/bin/x'';",
        );
        assert_eq!(loose_paths(&text), vec![String::from("enterShell")]);
    }

    #[test]
    fn the_wrapper_argument_set_is_closed() {
        let declared = arguments(&module(GOOD)).expect("the real shape reads");
        let expected: BTreeSet<String> = WRAPPER_ARGUMENTS.iter().map(|a| String::from(*a)).collect();
        assert_eq!(declared, expected);

        // #402's seventh escape, and the three siblings a blocklist of that one spelling misses.
        for extra in [
            "checkPhase = \"true\";",
            "doCheck = false;",
            "checkInputs = [ ];",
            "derivationArgs = { };",
        ] {
            let text = GOOD.replace("inherit bashOptions text;", &format!("inherit bashOptions text; {extra}"));
            let declared = arguments(&module(&text)).expect("the call still reads");
            assert_ne!(declared, expected, "`{extra}` has to move the argument set");
        }
    }

    #[test]
    fn a_missing_or_duplicated_builder_is_a_refusal() {
        let none = arguments(&module("{ enterTest = \"echo\"; }")).expect_err("no builder refuses");
        assert!(none.contains(LINTER), "{none}");
        let twice = module(
            "let\n  a = pkgs.writeShellApplication { text = \"x\"; };\n  \
             b = pkgs.writeShellApplication { text = \"y\"; };\nin\n{ }",
        );
        let two = arguments(&twice).expect_err("two builders refuse");
        assert!(two.contains("two wrappers"), "{two}");
    }

    #[test]
    fn an_import_this_gate_cannot_read_is_a_refusal() {
        let followed = imports("devenv.nix", &scan::assignments("{ imports = [ ./nix/dev-scripts.nix ]; }")).expect("a path");
        assert_eq!(followed, vec![String::from("nix/dev-scripts.nix")]);
        // Relative to the IMPORTING module, so a module two levels down resolves.
        let nested = imports("nix/a.nix", &scan::assignments("{ imports = [ ./b.nix ]; }")).expect("a path");
        assert_eq!(nested, vec![String::from("nix/b.nix")]);
        let refused = imports("devenv.nix", &scan::assignments("{ imports = [ inputs.x.modules.y ]; }"))
            .expect_err("an unreadable import refuses");
        assert!(refused.contains("cannot read"), "{refused}");
    }

    #[test]
    fn a_body_in_an_imported_module_is_held_the_same_way() {
        // The sixth escape, end to end: the identical literal in a second module. The old rule
        // READ that file and declined to judge it, because its scope was one basename.
        let read = vec![
            Module {
                rel: String::from("devenv.nix"),
                assignments: scan::assignments(GOOD),
            },
            Module {
                rel: String::from("nix/dev-scripts.nix"),
                assignments: scan::assignments("{ scripts.planted.exec = \"for f in $(ls *.rs); do echo $f; done\"; }"),
            },
        ];
        let wrapped = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &wrapped));
        let loose: Vec<&str> = judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| row.body.module.as_str())
            .collect();
        assert_eq!(loose, vec!["nix/dev-scripts.nix"]);
    }

    #[test]
    fn the_real_tree_is_held_and_every_body_is_routed() {
        // The gate over this repository, which is what the sweep runs. Not a smoke test: the
        // floor is the arm that catches a scan which stopped reading, and a fixed number here
        // would rot - so what is asserted is that the set is non-trivial and NOTHING in it is
        // loose.
        let root = crate::repo::root().expect("the repo root");
        let read = super::modules(&root).expect("devenv.nix reads");
        let wrapped = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &wrapped));
        assert!(
            judged.rows().len() > 10,
            "only {} bodies in the real tree",
            judged.rows().len()
        );
        assert!(judged.gap().is_none(), "{:?}", judged.gap());
        let loose: Vec<String> = judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| format!("{}:{}", row.body.module, row.body.line))
            .collect();
        assert_eq!(loose, Vec::<String>::new());
    }
}
