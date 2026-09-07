//! Which files ARE the devenv module set, and the two places they are declared.
//!
//! A module of its own for the 1000-line cap's reason, and the seam is a real one: [`super`] holds
//! the RULES over a module's assignments, this holds the question of which modules there are. The
//! answer was wrong twice, in the same direction both times, so it is worth its own file:
//! `github.com/telekom/sutura#402`'s sixth escape was the same literal in a module reached through
//! the Nix `imports` attribute, and the review of the fix found `devenv.yaml`'s own `imports:`
//! list - which devenv 2.2.2 loads beside it - read by nothing.
//!
//! **Fails closed at every entry.** An unreadable module, an import naming anything but a relative
//! path, and a directory with no `devenv.nix` in it are each a refusal rather than a narrowing: a
//! module this cannot read is a module whose shell bodies are held by nothing, which is the defect
//! the whole gate exists to close.

use std::collections::BTreeSet;
use std::path::Path;

use super::scan::{self, Assignment};

/// The devenv module every other one is reached from.
pub(super) const ROOT_MODULE: &str = "devenv.nix";

/// devenv's other module declaration site: its `imports:` list is loaded beside the Nix one.
const YAML: &str = "devenv.yaml";

/// One devenv module, read.
pub(super) struct Module {
    /// Repo-relative path, for a message a reader can open.
    pub(super) rel: String,
    /// Every assignment in it.
    pub(super) assignments: Vec<Assignment>,
}

/// Read `devenv.nix` and every module its `imports` reach.
///
/// FAILS CLOSED on anything it cannot read, and that is deliberate rather than defensive: #402's
/// sixth escape was the identical literal in an imported module, which the old rule READ and
/// declined to judge. A module this cannot follow is the same hole with a different cause, so it
/// is a refusal naming the entry rather than a silent narrowing.
pub(super) fn modules(root: &Path) -> Result<Vec<Module>, String> {
    let mut queue = vec![String::from(ROOT_MODULE)];
    queue.extend(yaml_imports(root)?);
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

/// The modules `devenv.yaml`'s own `imports:` list names.
///
/// **THE SECOND DECLARATION SITE, and the first version of this gate read only the first.** devenv
/// 2.2.2's evaluator takes `devenv_imports ? [ ]` and flattens it into the module list beside the
/// Nix `imports` attribute, so a module declared here is loaded exactly the same way - and a bare
/// body in one left the verdict at `20 of 20 ... in 1 module(s)`, exit 0, with the module count
/// the only tell and nothing comparing it.
///
/// An entry naming a relative path is followed; a directory means that directory's `devenv.nix`,
/// which is devenv's own rule. Anything else - an input name, a subdirectory of an input - is a
/// module this gate cannot read, and is refused for the same reason an unfollowable Nix import is.
/// An absent `devenv.yaml` is not an error: devenv does not require one.
pub(super) fn yaml_imports(root: &Path) -> Result<Vec<String>, String> {
    let path = root.join(YAML);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("could not read {YAML}: {error}"))?;
    let mut found = Vec::new();
    let mut inside = false;
    for (number, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("imports:") {
            inside = true;
            // The inline form, `imports: [ a, b ]`.
            for entry in rest.trim().trim_start_matches('[').trim_end_matches(']').split(',') {
                found.push(resolved(entry.trim(), root, number.saturating_add(1))?);
            }
            continue;
        }
        if !inside {
            continue;
        }
        match line.strip_prefix("- ") {
            Some(entry) => found.push(resolved(entry.trim(), root, number.saturating_add(1))?),
            // The list ended: the next key at any indentation is not one of its entries.
            None => inside = !line.is_empty() && !line.contains(':'),
        }
    }
    found.retain(|entry| !entry.is_empty());
    Ok(found)
}

/// One `devenv.yaml` import entry, as a repo-relative module path.
pub(super) fn resolved(entry: &str, root: &Path, line: usize) -> Result<String, String> {
    let entry = entry.trim().trim_matches('"').trim_matches('\'');
    if entry.is_empty() {
        return Ok(String::new());
    }
    let Some(relative) = entry.strip_prefix("./") else {
        return Err(format!(
            "{YAML}:{line}: `imports` names `{entry}`, which is not a relative path this gate can \
             read - devenv loads it as a module all the same, so the shell bodies in it would be \
             held by nothing. Import a relative path, or state the module in {ROOT_MODULE}"
        ));
    };
    if is_nix(relative) {
        return Ok(String::from(relative));
    }
    let module = format!("{relative}/{ROOT_MODULE}");
    if root.join(&module).is_file() {
        return Ok(module);
    }
    Err(format!(
        "{YAML}:{line}: `imports` names `{entry}`, and neither it nor `{module}` is a file this \
         gate can read"
    ))
}

/// Is this token a Nix file? Through `Path::extension`, which is what the workspace's lint set
/// asks for in place of a suffix compare.
pub(super) fn is_nix(token: &str) -> bool {
    Path::new(token)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("nix"))
}

/// The modules one module's `imports` names, as repo-relative paths.
///
/// Only a relative path literal is followed, and anything else in the list is a refusal: an
/// import that resolves through a flake input is a module this gate cannot read, and a gate that
/// silently holds less than the tree is the defect it exists to close.
pub(super) fn imports(rel: &str, assignments: &[Assignment]) -> Result<Vec<String>, String> {
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

#[cfg(test)]
mod tests {
    use super::{imports, resolved, yaml_imports};

    // THE HARNESS MOVED, THE ASSERTIONS DID NOT. `xtask/src/falsifier.rs`'s header records why a
    // moved file needs a `#[test]` of its own: a file adding none is revertible, the parent that
    // declares `mod modules;` is held at HEAD for ITS tests, and `E0583` then turns a real causal
    // verdict into `INCONCLUSIVE`. These two were in `super`'s test module before the 1000-line
    // cap forced this split.

    #[test]
    fn an_import_this_gate_cannot_read_is_a_refusal() {
        let followed = imports(
            "devenv.nix",
            &super::super::scan::assignments("{ imports = [ ./nix/dev-scripts.nix ]; }"),
        )
        .expect("a path");
        assert_eq!(followed, vec![String::from("nix/dev-scripts.nix")]);
        let nested = imports("nix/a.nix", &super::super::scan::assignments("{ imports = [ ./b.nix ]; }")).expect("a path");
        assert_eq!(nested, vec![String::from("nix/b.nix")]);
        let refused = imports(
            "devenv.nix",
            &super::super::scan::assignments("{ imports = [ inputs.x.modules.y ]; }"),
        )
        .expect_err("an unreadable import refuses");
        assert!(refused.contains("cannot read"), "{refused}");
    }

    #[test]
    fn a_yaml_import_this_gate_cannot_read_is_a_refusal() {
        // MUTATION OF THE MODULE-SET READER. devenv 2.2.2 loads `devenv.yaml`'s `imports:` beside
        // the Nix attribute, and reading only the Nix one left a bare body in such a module at
        // `20 of 20 ... in 1 module(s)`, exit 0 - the module count the only tell, compared to
        // nothing.
        let root = crate::repo::root().expect("the repo root");
        let refusal = resolved("nixpkgs-python", &root, 3).expect_err("an input name is not a module");
        assert!(refusal.contains("not a relative path"), "{refusal}");
        let missing = resolved("./nowhere", &root, 3).expect_err("a directory with no devenv.nix refuses");
        assert!(missing.contains("nowhere/devenv.nix"), "{missing}");
        // A relative `.nix` path resolves without touching the filesystem.
        assert_eq!(resolved("./nix/x.nix", &root, 3).expect("a path"), "nix/x.nix");
        // And the real file declares no imports, so the queue it contributes is empty.
        assert!(yaml_imports(&root).expect("devenv.yaml reads").is_empty());
    }
}
