//! `cargo xtask <name>` mentioned anywhere, and `just <name>` in a backtick span, must be a task
//! that exists (`github.com/telekom/sutura#1277`). A `just` citation outside backticks is not read.
//!
//! Its own module for `citations`' reason: `max-lines` caps a file at 1000 and cannot exempt
//! anything under `xtask/`. It reads every file it is handed - [`in_scope`](super::in_scope) is the
//! scope - where it once re-filtered to four of that list's six extensions and so never read a
//! `.sh` or `.toml` citation of a deleted task.

pub(super) fn bad_task_references(read: &crate::causality::regions::PostImage<'_>, files: &[String]) -> Vec<String> {
    let known = super::known_tasks();
    let mut problems = Vec::new();
    for rel in files {
        let Some(text) = read(rel) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            for marker in ["cargo xtask ", "-p xtask -- "] {
                let mut rest = line;
                while let Some(at) = rest.find(marker) {
                    let tail = rest.get(at + marker.len()..).unwrap_or("");
                    if let Some(name) = super::task_name_at(tail)
                        && !known.contains(name)
                    {
                        problems.push(format!(
                            "{rel}:{}: `{name}` is not an xtask task - it was renamed or deleted",
                            i + 1
                        ));
                    }
                    rest = tail;
                }
            }
        }
    }
    problems
}

/// `just <name>` in a backtick span of any in-scope file must be a recipe the justfile declares.
pub(super) fn bad_recipe_references(
    root: &std::path::Path,
    read: &crate::causality::regions::PostImage<'_>,
    files: &[String],
) -> Vec<String> {
    let Some(recipes) = crate::tasks::recipe_names(root) else {
        return vec![String::from("justfile could not be read to check `just` citations")];
    };
    let mut problems = Vec::new();
    for rel in files {
        let Some(text) = read(rel) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let n = i.saturating_add(1);
            for span in super::spans(line) {
                if let Some(super::advice::Cited::Recipe(name)) = super::advice::cited(span)
                    && !recipes.contains(name)
                {
                    problems.push(format!(
                        "{rel}:{n}: `just {name}` is not a just task - it was renamed or deleted"
                    ));
                }
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_deleted_task_cited_in_a_shell_script_is_flagged() {
        // `bad_task_references` once re-filtered to md/nix/yaml/yml inside `in_scope`'s toml and sh,
        // so a `cargo xtask <name>` citation in a .sh file was invisible to the check.
        let read = |rel: &str| match rel {
            "nix/run-gate.sh" => Some(String::from("#!/usr/bin/env bash\ncargo xtask no-such-gate --since HEAD\n")),
            _ => None,
        };
        let files = vec![String::from("nix/run-gate.sh")];
        let problems = super::bad_task_references(&read, &files);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no-such-gate") && p.contains("nix/run-gate.sh")),
            "a deleted task cited in a .sh file must be flagged: {problems:?}"
        );
    }

    #[test]
    fn a_deleted_recipe_cited_in_prose_is_flagged() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repo root");
        let read = |rel: &str| match rel {
            "docs/example.md" => Some(String::from("run `just no-such-task-xyz` first")),
            _ => None,
        };
        let files = vec![String::from("docs/example.md")];
        let problems = super::bad_recipe_references(root, &read, &files);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("no-such-task-xyz") && p.contains("docs/example.md")),
            "a deleted just task cited in prose must be flagged: {problems:?}"
        );
    }

    #[test]
    fn a_live_recipe_cited_in_prose_is_not_flagged() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repo root");
        let read = |rel: &str| match rel {
            "docs/example.md" => Some(String::from("run `just lint` first")),
            _ => None,
        };
        let files = vec![String::from("docs/example.md")];
        let problems = super::bad_recipe_references(root, &read, &files);
        assert!(
            problems.is_empty(),
            "a live just task citation must not be flagged: {problems:?}"
        );
    }

    #[test]
    fn a_flag_is_not_read_as_a_deleted_recipe() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the repo root");
        let read = |rel: &str| match rel {
            "docs/example.md" => Some(String::from("run `just --list` first")),
            _ => None,
        };
        let files = vec![String::from("docs/example.md")];
        let problems = super::bad_recipe_references(root, &read, &files);
        assert!(
            problems.is_empty(),
            "`just --list` is a flag, not a deleted task: {problems:?}"
        );
    }
}
