//! `cargo fmt`, scoped to the packages we actually own.
//!
//! WHY THIS EXISTS AND IS NOT JUST `cargo fmt --all`.
//!
//! `--all` formats every package `cargo metadata` reports, and that includes **path
//! dependencies which are not workspace members**. `[workspace] exclude` keeps the vendored
//! mimalloc crates out of the member list, and `--all` reaches them anyway - it wanted to
//! rewrite upstream's `build.rs` and `lib.rs`, which is the one thing vendoring must never do.
//! Reformatted vendored source no longer matches the release it claims to be, and every future
//! upstream sync inherits the noise.
//!
//! rustfmt's own `ignore` key would be the obvious fix and cannot be used: it is nightly-only,
//! and on stable it prints `can't set ignore, unstable features are only available in nightly`
//! and carries on formatting.
//!
//! So the package list is DERIVED from `cargo metadata`'s `workspace_members` rather than
//! written down. A hardcoded `-p a -p b` in the hook would work today and silently stop
//! covering a crate the day one is added, which is the failure this repo treats as worse than
//! the bug it prevents.

use crate::Verdict;

/// Every workspace member's package name, from `cargo metadata`.
///
/// `--no-deps` because the member list is the question; resolving the dependency graph would
/// be slower and would reintroduce exactly the non-member packages we are excluding.
fn member_names() -> Result<Vec<String>, String> {
    let metadata = crate::cargo_metadata(&["--no-deps"])?;

    // `workspace_members` holds package IDs, and their format is not guaranteed stable across
    // cargo versions. `packages` under `--no-deps` is already exactly the member set, so the
    // names come from there and no ID parsing is needed.
    let packages = metadata
        .get("packages")
        .and_then(|p| p.as_array())
        .ok_or_else(|| String::from("cargo metadata had no `packages` array"))?;

    let mut names: Vec<String> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect();
    names.sort();
    names.dedup();

    if names.is_empty() {
        return Err(String::from("cargo metadata reported no workspace packages"));
    }
    Ok(names)
}

/// `xtask fmt [--check]` - format the workspace's own packages, and nothing vendored.
pub(crate) fn run(args: &[String]) -> Verdict {
    let check = args.iter().any(|a| a == "--check");

    let names = match member_names() {
        Ok(names) => names,
        Err(message) => {
            eprintln!("xtask fmt: {message}");
            return Verdict::Fail;
        }
    };

    let mut command = std::process::Command::new("cargo");
    command.arg("fmt");
    for name in &names {
        command.args(["-p", name]);
    }
    if check {
        // `--` separates cargo-fmt's own arguments from rustfmt's.
        command.args(["--", "--check"]);
    }

    match command.status() {
        Ok(status) if status.success() => {
            let verb = if check { "checked" } else { "formatted" };
            println!("xtask fmt: {verb} {} package(s): {}", names.len(), names.join(", "));
            Verdict::Pass
        }
        Ok(_) => {
            if check {
                eprintln!();
                eprintln!("xtask fmt: run `just fmt` to apply this.");
            }
            Verdict::Fail
        }
        Err(error) => {
            eprintln!("xtask fmt: could not run cargo fmt: {error}");
            Verdict::Fail
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_member_list_is_derived_and_excludes_vendored_packages() {
        // The property that matters: `[workspace] exclude` keeps the vendored crates out, so a
        // derived list cannot reach them. If someone removes the exclude, this fails - which is
        // the point, because `cargo fmt` would then rewrite upstream source.
        let names = super::member_names().expect("this workspace has members");
        assert!(names.contains(&String::from("xtask")), "{names:?}");
        for vendored in ["mimalloc", "libmimalloc-sys"] {
            assert!(
                !names.contains(&String::from(vendored)),
                "`{vendored}` is vendored and must not be in the formatter's scope: {names:?}"
            );
        }
    }
}
