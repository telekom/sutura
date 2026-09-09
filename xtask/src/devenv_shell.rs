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
//!    transitively, so `runs` and `sourced` are wrappers because they reach `linted`.
//!    A binding that only looks like one is not.
//! 2. **The scan is fail-closed at both ends.** An unreadable module, an `imports` entry this
//!    cannot follow, no wrapper at all, a wrapper NAME bound twice, and a discovery pass that
//!    found NOTHING are each a refusal. #402's own measurement of this repository's gates is the
//!    argument: over a whole-repo falsifier tree only 3 of 31 refused because their own rule
//!    fired, 20 refused on a missing input and 8 on an empty-scan floor - so a gate wants all
//!    three arms, and wants to say which one answered.
//! 3. **The wrapper's argument set is CLOSED, over EVERY attrset in the call.** #402's seventh
//!    escape defeated the linter rather than the rule: `checkPhase = "true";` added to `linted`
//!    removes `bash -n` AND `shellcheck` from every body, and every textual gate stayed green
//!    because they held the spelling `runs`/`sourced` rather than the emission. A blocklist of
//!    that one attribute would not have closed it either: `doCheck`, `checkInputs` and
//!    `derivationArgs` each do the same job. Reading only the FIRST balanced attrset did not
//!    close it either - `{ ... } // { checkPhase = "true"; }` and
//!    `.overrideAttrs (_: { checkPhase = "true"; })` were both measured green with the verdict
//!    still printing the closed set - so every attrset at every depth is read, and a call naming
//!    `//` or [`PAST_THE_ARGUMENTS`] is refused outright. See [`WRAPPER_ARGUMENTS`].
//!
//! # The limits, next to the claims
//!
//! * **This gate is structural. It does not run `shellcheck`**, and cannot: the sweep it is in
//!   runs inside a nix derivation with no nix. The EMISSION is read by
//!   `cargo xtask check-devenv-linter`, which needs a store the wrapper has been built into -
//!   `just devenv-linter` is that venue, and `just ship-check` runs it when a diff touches a
//!   devenv module. So on a pull request the structural half runs and the linter half does not.
//! * **A one-line string body under an attribute name this does not know is unheld.** Rule 1 keys
//!   on [`SHELL_ATTRIBUTES`], which is devenv's shell options and not a list nix can derive here.
//!   The third arm catches a multi-line `''` literal at any depth and under any name, because a
//!   shell body of any size is a block; what falls between is a one-liner under a NEW option. A
//!   `''` literal that belongs to no assignment - a bare element of a list - is unheld too, and
//!   that is the price of not counting every enclosing set as a body of its own.
//! * **A module devenv loads and git does not publish is invisible.** `devenv.local.nix` is
//!   gitignored by design, so it is neither read nor held. Both declaration sites git DOES publish
//!   are read: `devenv.nix`'s `imports` attribute and `devenv.yaml`'s `imports:` list.
//! * **A wrapper is admitted by its vocabulary, so a legitimate one reaching for a nixpkgs helper
//!   is refused until that name is added.** The direction is deliberate: the bodies through it
//!   then read as loose and the failure names the identifier that kept it out. Nix SCOPING is not
//!   modelled at all - a name bound twice is refused rather than resolved.
//! * **A body reaching a wrapper through a conditional is not followed.** A text scan cannot; what
//!   replaces following it is the vocabulary rule, which refuses the binding instead.

use std::collections::{BTreeMap, BTreeSet};

use crate::Verdict;
use crate::repo;
use scan::{Assignment, Value};
use tally::{Body, Discovered, Held, Judged};

mod scan;
mod tally;

/// The devenv options whose value is a shell body.
///
/// Written down because nothing here can derive it - devenv's option set lives in its own flake,
/// and this gate runs where there is no nix. `exec` covers `scripts`, `tasks` and `processes`
/// alike, which is the whole reason the attribute NAME is the key: the path in front of it is
/// what the six measured spellings differed in.
/// `status` is `tasks.<name>.status`, which devenv 2.2.2 turns into a script through
/// `pkgs.writeScript` - a shell body under a name nothing else here would have looked at.
const SHELL_ATTRIBUTES: &[&str] = &["enterShell", "enterTest", "exec", "startupCommand", "status"];

/// The only attributes the wrapper may hand `writeShellApplication`.
///
/// A CLOSED set, and that is the whole of rule 3. Every way to remove the linter is an attribute
/// passed through to the derivation: `checkPhase` replaces the phase, `doCheck` skips it,
/// `checkInputs` empties its tool set, `derivationArgs` carries any of those one level down. A
/// blocklist would be a list of spellings to keep complete; this is the complement, so a
/// wrapper that grows an argument is a visible diff a reviewer has to agree with.
///
/// `extraShellCheckFlags` is the fourth element and a DELIBERATE widening: the pinned builder
/// interpolates it into the DEFAULT phase, so it adds a flag without replacing anything - which
/// is how these bodies get `-x`, the flag the tracked `*.sh` files were already getting. Its
/// sibling `excludeShellChecks` is NOT admitted: that one removes findings.
const WRAPPER_ARGUMENTS: &[&str] = &["name", "bashOptions", "text", "extraShellCheckFlags"];

/// Ways to reach the derivation past the argument set, which is what makes reading the argument
/// set enough. Named in a call, they are a refusal.
///
/// `{ ... } // { checkPhase = "true"; }` and `.overrideAttrs (_: { checkPhase = "true"; })` both
/// move the effective phase; the first version of this gate read only the FIRST balanced attrset
/// in the call and printed the three-element set as though it were the whole argument list, with
/// `hygiene` green over a body nothing had read. Refused rather than parsed, because a merge whose
/// operands come from anywhere is not something a text scan can bound.
const PAST_THE_ARGUMENTS: &[&str] = &["overrideAttrs", "overrideDerivation"];

/// Everything a wrapper's value may name besides its own parameters, another wrapper and the
/// builder.
///
/// The reason this is a list rather than a blocklist of script builders: a wrapper that reaches a
/// wrapper on one branch and `pkgs.writeShellScriptBin` on another was ADMITTED by a growth rule
/// that only asked whether a wrapper was mentioned, and the verdict then named it as the wrapper
/// that held a body. Enumerating the builders it must not name is a list to keep complete; naming
/// what it MAY use is not. An addition here is a deliberate widening a reviewer sees.
const WRAPPER_VOCABULARY: &[&str] = &[
    "if", "then", "else", "let", "in", "with", "inherit", "rec", "assert", "or", "true", "false", "null", "builtins", "import",
    "toString",
];

/// The nixpkgs builder whose `checkPhase` is the linter. The wrapper set is seeded from it.
const LINTER: &str = "writeShellApplication";

mod modules;

use modules::Module;

/// The wrapper set, and why each candidate that did not make it did not.
struct Chain {
    /// Bindings whose value reaches the linter and nothing else.
    admitted: BTreeSet<String>,
    /// A binding that mentions a wrapper and was refused, with the identifier that refused it.
    /// Read by [`loose`], so a body assigned through such a name says WHY rather than only that
    /// it was not routed.
    turned_away: BTreeMap<String, String>,
}

/// Which `let` bindings reach the linter, transitively - and only the linter.
///
/// DERIVED rather than named, which is what makes a new wrapper work the day it is written.
/// Seeded from the bindings that mention [`LINTER`], so replacing `writeShellApplication` with a
/// builder that has no `checkPhase` empties the set, and the empty set is a refusal.
///
/// **MENTIONING A WRAPPER IS NOT ROUTING THROUGH ONE, and that was a live escape**:
/// `hybrid = name: body: if false then runs name body else "${pkgs.writeShellScriptBin name
/// body}/bin/${name}";` was admitted at exit 0 and the verdict named it as the wrapper that held a
/// body. So a candidate is admitted only if EVERY identifier its value uses is one of its own
/// parameters, an already-admitted wrapper, the builder, or [`WRAPPER_VOCABULARY`]. A closed
/// vocabulary rather than a blocklist of script builders, for the reason that constant states.
fn wrappers(modules: &[Module]) -> Chain {
    let bindings: Vec<&Assignment> = modules
        .iter()
        .flat_map(|module| module.assignments.iter())
        .filter(|a| a.lets > 0)
        .collect();

    let mut admitted = BTreeSet::new();
    let mut turned_away = BTreeMap::new();
    loop {
        let mut grown = false;
        for binding in &bindings {
            if admitted.contains(&binding.attribute) {
                continue;
            }
            let reaches = scan::mentions(&binding.code, LINTER)
                || admitted
                    .iter()
                    .any(|known: &String| known != &binding.attribute && scan::mentions(&binding.code, known));
            if !reaches {
                continue;
            }
            if let Some(stranger) = unknown_identifier(binding, &admitted) {
                turned_away.insert(binding.attribute.clone(), stranger);
            } else {
                turned_away.remove(&binding.attribute);
                admitted.insert(binding.attribute.clone());
                grown = true;
            }
        }
        if !grown {
            return Chain { admitted, turned_away };
        }
    }
}

/// The first identifier in `binding`'s value that a wrapper may not use.
fn unknown_identifier(binding: &Assignment, admitted: &BTreeSet<String>) -> Option<String> {
    scan::identifiers(&binding.code).into_iter().find(|token| {
        let last = token.rsplit('.').next().unwrap_or(token);
        !binding.params.contains(token)
            && !admitted.contains(token)
            && last != LINTER
            && !WRAPPER_VOCABULARY.contains(&token.as_str())
    })
}

/// A wrapper name bound more than once, if there is one.
///
/// **A NAME is the whole of rule 1, so a name that means two things defeats it.** Bind
/// `runs = name: body: body;` in a nested `let` and a body assigned through it reads as
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

    // Reaching the derivation past the argument set makes reading that set say nothing, so it is
    // refused ahead of reading it.
    for escape in PAST_THE_ARGUMENTS {
        if scan::mentions(&call.code, escape) {
            return Err(format!(
                "the `{LINTER}` call names `{escape}`, which reaches the derivation past its \
                 argument set - so reading that set says nothing about the phase. Pass what the \
                 body needs as an argument, or this gate cannot hold the emission"
            ));
        }
    }
    if call.code.contains("//") {
        return Err(format!(
            "the `{LINTER}` call merges with `//` - the operands can come from anywhere, so the \
             argument set is not readable here. `{{ ... }} // {{ checkPhase = \"true\"; }}` was \
             measured green with the verdict still naming the closed set"
        ));
    }

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

/// The `;`-separated members of EVERY attrset in `code`, at every depth.
///
/// Not the first balanced one, which is what it used to be and is the defect that made
/// `{ ... } // { checkPhase = "true"; }` invisible while the verdict printed the three-element
/// set. Depth-keyed buffers rather than one, so a nested attrset's members belong to it and not
/// to its parent.
fn members(code: &str) -> Result<Vec<String>, String> {
    let mut buffers: Vec<String> = Vec::new();
    let mut segments = Vec::new();
    let mut seen = false;
    // `${...}` is an INTERPOLATION, not an attrset, and the lexer keeps both braces - so reading
    // it as one made `name = "sutura-${name}"` contribute a member called `name` with no `=`.
    // Counted as an interpolation depth instead, whose interior belongs to the enclosing member.
    let mut interpolations = 0_usize;
    let mut previous = ' ';
    for c in code.chars() {
        let opens_interpolation = c == '{' && previous == '$';
        previous = c;
        match c {
            '{' if opens_interpolation => {
                interpolations = interpolations.saturating_add(1);
                if let Some(open) = buffers.last_mut() {
                    open.push(c);
                }
            }
            '}' if interpolations > 0 => {
                interpolations = interpolations.saturating_sub(1);
                if let Some(open) = buffers.last_mut() {
                    open.push(c);
                }
            }
            '{' => {
                seen = true;
                buffers.push(String::new());
            }
            '}' => match buffers.pop() {
                Some(open) => segments.extend(open.split(';').map(String::from)),
                None => {
                    return Err(format!(
                        "the `{LINTER}` application's braces do not balance, so its argument set \
                         is not readable: `{}`",
                        code.trim()
                    ));
                }
            },
            _ => {
                if let Some(open) = buffers.last_mut() {
                    open.push(c);
                }
            }
        }
    }
    if !buffers.is_empty() {
        return Err(format!(
            "the `{LINTER}` application has an attrset that never closes: `{}`",
            code.trim()
        ));
    }
    if !seen {
        return Err(format!(
            "the `{LINTER}` application has no attrset this gate can read: `{}`",
            code.trim()
        ));
    }
    Ok(segments)
}

/// Every shell-bearing assignment in the module set.
///
/// THREE reasons an assignment is picked up, and each is stated on the body because their limits
/// differ.
///
/// * A [`SHELL_ATTRIBUTES`] name, **at module level only** - a `let` binding is not a devenv
///   option, and `crate::workflows::code_lines`' `lets` is what tells the two apart.
/// * A multi-line string literal as the whole value.
/// * A multi-line `''...''` literal **anywhere in the value's span, at any `let` depth**, which is
///   the reason the scope rule above is only on the first arm. `helper = pkgs.writeShellScriptBin
///   "helper" ''...'';` in a `let` block projects to an APPLICATION rather than to a literal, so
///   the second arm cannot see it, and a `lets == 0` filter took the whole `let` block out of
///   reach - measured at exit 0 over an unlinted body.
fn discover(modules: &[Module]) -> Discovered {
    let mut bodies = Vec::new();
    for module in modules {
        for assignment in &module.assignments {
            let named = assignment.lets == 0 && SHELL_ATTRIBUTES.contains(&assignment.attribute.as_str());
            let because = if named {
                "a devenv option whose value is a shell body"
            } else if matches!(assignment.value, Value::Literal { lines } if lines > 1) {
                "a multi-line string literal, which is the shape of a shell body"
            } else if assignment.indented && !matches!(assignment.value, Value::Structure) {
                // NOT a set or a list: `scripts = { fmt.exec = ...; }` spans every body inside it,
                // and those are assignments of their own that this loop reaches separately.
                // Counting the enclosing set as well would report it as a loose body and make the
                // discovered count a tree walk rather than a set of bodies. The cost is stated in
                // the module header: a `''` literal as a bare LIST element belongs to no
                // assignment and is unheld.
                "an assignment carrying a multi-line `''` literal, whatever it is assigned to"
            } else {
                continue;
            };
            bodies.push(Body {
                module: module.rel.clone(),
                line: assignment.line,
                path: assignment.path.clone(),
                attribute: assignment.attribute.clone(),
                value: assignment.value.clone(),
                applied: assignment.applied.clone(),
                because,
            });
        }
    }
    Discovered::of(bodies)
}

/// Did this body's value go through a wrapper?
///
/// Keyed on what the value APPLIES rather than on what it begins with, and the difference is a
/// measured escape: the head of `name: body: runs name ''...''` is the parameter `name`, so a
/// wrapper's own definition read as unrouted while a lambda hiding a builder read as routed.
fn judge(body: &Body, wrapped: &BTreeSet<String>) -> Held {
    match &body.applied {
        Some(applied) if wrapped.contains(applied) => Held::Routed(applied.clone()),
        _ => Held::Loose,
    }
}

/// What a loose body's failure line says.
fn loose(body: &Body, chain: &Chain) -> String {
    let through = chain.admitted.iter().cloned().collect::<Vec<String>>().join("`, `");
    let shape = match (&body.applied, &body.value) {
        // The sharpest case to read: a binding that DOES mention a wrapper and was refused. Saying
        // only "reaches no wrapper" would send a reader to the call site rather than to the cause.
        (Some(applied), _) if chain.turned_away.contains_key(applied) => format!(
            "it applies `{applied}`, which this gate refused as a wrapper because that binding \
             also names `{}` - so which of the two a body reaches is not readable here",
            chain.turned_away.get(applied).map_or("", String::as_str)
        ),
        (Some(applied), _) => format!("it applies `{applied}`, which reaches no wrapper"),
        (None, Value::Structure) => String::from("its value is a set or a list, so no wrapper saw the body"),
        (None, Value::Literal { lines }) => {
            format!("its value is a bare string literal over {lines} line(s)")
        }
        (None, Value::Head(head)) => format!("its value begins with `{head}` and applies nothing"),
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

    let read = match modules::modules(&root) {
        Ok(read) => read,
        Err(reason) => return refuse("a module it could not read", &[reason]),
    };

    let chain = wrappers(&read);
    let wrapped = chain.admitted.clone();
    if wrapped.is_empty() {
        return refuse(
            "its own rule",
            &[format!(
                "no `let` binding in any devenv module reaches `{LINTER}` and only `{LINTER}` - so \
                 no body is read by ShellCheck, whatever it is assigned through"
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
            .map(|row| loose(&row.body, &chain)),
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

    use super::{Held, Judged, LINTER, Module, WRAPPER_ARGUMENTS, arguments, discover, judge, scan, wrappers};

    /// A devenv module with the real shapes: the wrapper chain in a `let`, and wrapped bodies.
    const GOOD: &str = r#"{ pkgs, ... }:
let
  linted = name: bashOptions: text:
    pkgs.writeShellApplication { name = "sutura-${name}"; inherit bashOptions text; extraShellCheckFlags = [ "-x" ]; };
  runs = name: body: "${linted name [ "errexit" ] body}/bin/sutura-${name}";
  sourced = name: body: "source ${linted name [ ] body}/bin/sutura-${name}";
in
{
  env.PLAIN = "not shell";
  enterShell = sourced "enter-shell" ''
    echo hello
  '';
  scripts = {
    fmt.exec = runs "fmt" ''
      cargo run -q -p xtask -- fmt
    '';
    secrets.exec = runs "secrets" "betterleaks dir .";
  };
}
"#;

    /// The one line in [`GOOD`] a planted body replaces.
    const ANCHOR: &str = "  env.PLAIN = \"not shell\";";

    /// The `let` line a planted binding replaces.
    const LET_ANCHOR: &str = "  sourced = name: body: \"source ${linted name [ ] body}/bin/sutura-${name}\";";

    fn module(text: &str) -> Vec<Module> {
        vec![Module {
            rel: String::from("devenv.nix"),
            assignments: scan::assignments(text),
        }]
    }

    /// Every body in `text` the gate would refuse, by assigned path.
    fn loose_paths(text: &str) -> Vec<String> {
        let read = module(text);
        let chain = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &chain.admitted));
        judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| row.body.path.clone())
            .collect()
    }

    #[test]
    fn the_wrapper_chain_is_derived_from_the_builder_and_not_listed() {
        let chain = wrappers(&module(GOOD));
        for expected in ["linted", "runs", "sourced"] {
            assert!(chain.admitted.contains(expected), "{expected}: {:?}", chain.admitted);
        }
        // A binding that mentions nothing is not a wrapper, and neither is one this gate has no
        // vocabulary for.
        let faked = wrappers(&module(
            "let\n  linted = n: b: t: pkgs.writeShellApplication { };\n  fake = n: b: b;\nin\n{ }",
        ));
        assert!(faked.admitted.contains("linted"));
        assert!(!faked.admitted.contains("fake"), "{:?}", faked.admitted);
    }

    #[test]
    fn a_binding_that_mentions_a_wrapper_and_also_a_builder_is_not_one() {
        // MUTATION OF THE GROWTH READER. The rule used to be *mentions a wrapper name*, which
        // admitted a binding reaching a wrapper on one branch and a builder with no `checkPhase`
        // on the other - and the verdict then named it as the wrapper that held a body.
        let hybrid = "  hybrid = name: body: if false then runs name body \
                      else \"${pkgs.writeShellScriptBin name body}/bin/${name}\";";
        let text = GOOD
            .replace(LET_ANCHOR, &format!("{LET_ANCHOR}\n{hybrid}"))
            .replace(ANCHOR, "  scripts.planted.exec = hybrid \"planted\" \"echo bad\";");
        let chain = wrappers(&module(&text));
        assert!(!chain.admitted.contains("hybrid"), "{:?}", chain.admitted);
        assert_eq!(
            chain.turned_away.get("hybrid").map(String::as_str),
            Some("pkgs.writeShellScriptBin"),
            "the refusal has to name what kept it out: {:?}",
            chain.turned_away
        );
        // And the body assigned through it is refused, with the cause rather than only the effect.
        assert_eq!(loose_paths(&text), vec![String::from("scripts.planted.exec")]);
        let body = super::Body {
            module: String::from("devenv.nix"),
            line: 1,
            path: String::from("scripts.planted.exec"),
            attribute: String::from("exec"),
            value: super::Value::Head(String::from("hybrid")),
            applied: Some(String::from("hybrid")),
            because: "a devenv option whose value is a shell body",
        };
        let said = super::loose(&body, &chain);
        assert!(said.contains("refused as a wrapper"), "{said}");
        assert!(said.contains("pkgs.writeShellScriptBin"), "{said}");
    }

    #[test]
    fn a_builder_with_no_checkphase_empties_the_wrapper_set() {
        let swapped = wrappers(&module(
            "let\n  linted = n: t: pkgs.writeShellScriptBin n t;\nin\n{ enterTest = linted \"a\" \"b\"; }",
        ));
        assert!(swapped.admitted.is_empty(), "{:?}", swapped.admitted);
    }

    #[test]
    fn every_measured_escape_is_a_loose_body() {
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
                "tasks.<name>.status, a str devenv turns into a script",
                "  tasks.\"x:y\".status = \"for f in $(ls *.rs); do echo $f; done\";",
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
    fn a_body_bound_in_the_let_block_is_discovered_too() {
        // MUTATION OF THE SCOPE READER. `lets == 0` is right for the option-name arm and was
        // wrong for the literal one: a body built by `writeShellScriptBin` inside `let ... in`
        // projects to an APPLICATION, so neither the name arm nor a whole-value literal saw it.
        let planted = format!(
            "{LET_ANCHOR}\n  helper = pkgs.writeShellScriptBin \"helper\" ''\n    for f in $(ls *.rs); do echo $f; done\n  '';"
        );
        let text = GOOD.replace(LET_ANCHOR, &planted);
        assert_eq!(loose_paths(&text), vec![String::from("helper")]);
        // And the real `let` bindings are not swept up with it: they carry `''` literals and
        // route through the wrappers, so they are discovered AND held.
        assert_eq!(loose_paths(GOOD), Vec::<String>::new());
    }

    #[test]
    fn the_real_shapes_are_not_flagged() {
        assert_eq!(loose_paths(GOOD), Vec::<String>::new());
    }

    #[test]
    fn a_body_glued_into_a_string_around_a_wrapper_is_still_loose() {
        let text = GOOD.replace(
            "  enterShell = sourced \"enter-shell\" ''\n    echo hello\n  '';",
            "  enterShell = ''source ${linted \"enter-shell\" [ ] ''echo hello''}/bin/x'';",
        );
        assert_eq!(loose_paths(&text), vec![String::from("enterShell")]);
    }

    #[test]
    fn the_wrapper_argument_set_is_closed_over_every_attrset() {
        let declared = arguments(&module(GOOD)).expect("the real shape reads");
        let expected: BTreeSet<String> = WRAPPER_ARGUMENTS.iter().map(|a| String::from(*a)).collect();
        assert_eq!(declared, expected);

        // #402's seventh escape written inside the first brace pair, plus the three siblings a
        // blocklist of that one spelling misses.
        for extra in [
            "checkPhase = \"true\";",
            "doCheck = false;",
            "checkInputs = [ ];",
            "derivationArgs = { };",
        ] {
            let text = GOOD.replace("inherit bashOptions text;", &format!("inherit bashOptions text; {extra}"));
            let moved = arguments(&module(&text)).expect("the call still reads");
            assert_ne!(moved, expected, "`{extra}` has to move the argument set");
        }
    }

    #[test]
    fn an_attribute_merged_on_past_the_first_attrset_is_refused() {
        // MUTATION OF THE ARGUMENT READER. It used to stop at the first balanced attrset, so both
        // of these left the verdict printing the closed set while the effective phase was `true`.
        let merged = GOOD.replace(
            "pkgs.writeShellApplication { name = \"sutura-${name}\"; inherit bashOptions text; extraShellCheckFlags = [ \"-x\" ]; };",
            "pkgs.writeShellApplication ({ name = \"sutura-${name}\"; inherit bashOptions text; extraShellCheckFlags = [ \"-x\" ]; } // { checkPhase = \"true\"; });",
        );
        let refusal = arguments(&module(&merged)).expect_err("a `//` merge is refused");
        assert!(refusal.contains("merges with `//`"), "{refusal}");

        let overridden = GOOD
            .replace("    pkgs.writeShellApplication {", "    (pkgs.writeShellApplication {")
            .replace(
                "extraShellCheckFlags = [ \"-x\" ]; };",
                "extraShellCheckFlags = [ \"-x\" ]; }).overrideAttrs (_: { checkPhase = \"true\"; });",
            );
        let refusal = arguments(&module(&overridden)).expect_err("overrideAttrs is refused");
        assert!(refusal.contains("overrideAttrs"), "{refusal}");

        // And a nested attrset that is NOT the first one still contributes its members, which is
        // what makes the refusals above a belt rather than the only reader.
        let nested = GOOD.replace(
            "extraShellCheckFlags = [ \"-x\" ];",
            "extraShellCheckFlags = [ \"-x\" ]; derivationArgs = { checkPhase = \"true\"; };",
        );
        let seen = arguments(&module(&nested)).expect("the call reads");
        assert!(seen.contains("checkPhase"), "a deeper attrset is read too: {seen:?}");
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
    fn a_wrapper_name_bound_twice_is_a_refusal_rather_than_a_guess() {
        let read = module(&GOOD.replace("in\n{", "  nested = let runs = n: b: b; in runs;\nin\n{"));
        let chain = wrappers(&read);
        let refusal = super::shadowed(&read, &chain.admitted).expect("a shadowed wrapper refuses");
        assert!(refusal.contains("`runs` is bound 2 times"), "{refusal}");
        let clean = module(GOOD);
        assert!(super::shadowed(&clean, &wrappers(&clean).admitted).is_none());
    }

    #[test]
    fn a_body_in_an_imported_module_is_held_the_same_way() {
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
        let chain = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &chain.admitted));
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
        // The gate over this repository, which is what the sweep runs. The floor is the arm that
        // catches a scan which stopped reading, and a fixed number here would rot - so what is
        // asserted is that the set is non-trivial and NOTHING in it is loose.
        let root = crate::repo::root().expect("the repo root");
        let read = crate::devenv_shell::modules::modules(&root).expect("devenv.nix reads");
        let chain = wrappers(&read);
        let judged = Judged::of(discover(&read), |body| judge(body, &chain.admitted));
        assert!(
            judged.rows().len() > 10,
            "only {} bodies in the real tree",
            judged.rows().len()
        );
        assert!(judged.gap().is_none(), "{:?}", judged.gap());
        assert!(chain.turned_away.is_empty(), "{:?}", chain.turned_away);
        let loose: Vec<String> = judged
            .rows()
            .iter()
            .filter(|row| matches!(row.held, Held::Loose))
            .map(|row| format!("{}:{}", row.body.module, row.body.line))
            .collect();
        assert_eq!(loose, Vec::<String>::new());
        // The argument set of the real wrapper, so `-x` cannot be dropped without a diff here.
        let declared = arguments(&read).expect("the real call reads");
        assert!(declared.contains("extraShellCheckFlags"), "{declared:?}");
    }
}
