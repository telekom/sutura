//! `cargo xtask <name>` mentioned anywhere must be a task that exists.
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
}
