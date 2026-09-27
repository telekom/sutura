//! Claim-mutation artifacts select the categories of their base and head targets.
//! Git parses without applying. Structural patches and unreadable revisions retain `core`.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Expand once: a patch targeting another artifact remains an unclassified path.
pub(super) fn expand(paths: &[String], root: &Path, base: Option<&str>) -> (Vec<String>, Vec<String>) {
    let mut expanded = Vec::new();
    let mut reasons = Vec::new();
    for path in paths {
        if !is_artifact(path) {
            expanded.push(path.clone());
            continue;
        }
        match targets(root, base, path) {
            Ok(targets) => expanded.extend(targets),
            Err(why) => {
                expanded.push(path.clone());
                reasons.push(format!("{path}: {why} - running every category"));
            }
        }
    }
    (expanded, reasons)
}

fn is_artifact(path: &str) -> bool {
    path.strip_prefix("devco/claim-mutations/")
        .and_then(|name| name.strip_suffix(".patch"))
        .is_some_and(|name| !name.is_empty() && !name.contains('/'))
}

fn git(root: &Path) -> Command {
    let mut command = Command::new("git");
    crate::repo::strip_git_env(&mut command);
    command.current_dir(root).arg("--literal-pathspecs");
    command
}

fn checked(output: std::io::Result<Output>, operation: &str) -> Result<Vec<u8>, String> {
    let output = output.map_err(|error| format!("{operation}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{operation}: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(output.stdout)
}

fn targets(root: &Path, base: Option<&str>, path: &str) -> Result<BTreeSet<String>, String> {
    let base = base.ok_or("no base revision for the mutation artifact")?;
    let metadata = std::fs::symlink_metadata(root.join(path)).map_err(|error| format!("read head metadata: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err(String::from("head mutation artifact is not a regular file"));
    }
    let head = std::fs::read(root.join(path)).map_err(|error| format!("read head mutation: {error}"))?;
    let mut paths = patch_targets(root, &head)?;
    if let Some(before) = base_patch(root, base, path)? {
        paths.extend(patch_targets(root, &before)?);
    }
    Ok(paths)
}

/// Only a successful exact tree lookup can establish that an artifact is new.
fn base_patch(root: &Path, base: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let tree = checked(
        git(root)
            .args(["rev-parse", "--verify", "--end-of-options", &format!("{base}^{{tree}}")])
            .output(),
        "resolve base tree",
    )?;
    let tree = std::str::from_utf8(&tree)
        .map_err(|error| format!("base tree name: {error}"))?
        .trim();
    if !object_name(tree) {
        return Err(String::from("base tree is not an object name"));
    }
    let entry = checked(
        git(root).args(["ls-tree", "-z", "--full-tree", tree, "--", path]).output(),
        "look up base mutation",
    )?;
    if entry.is_empty() {
        return Ok(None);
    }
    let entry = std::str::from_utf8(&entry).map_err(|error| format!("base tree entry: {error}"))?;
    let (header, name) = entry.split_once('\t').ok_or("base mutation entry has no path")?;
    if name.strip_suffix('\0') != Some(path) {
        return Err(String::from("base mutation lookup did not name exactly the artifact"));
    }
    let fields: Vec<_> = header.split(' ').collect();
    let [mode, "blob", object] = fields.as_slice() else {
        return Err(String::from("base mutation is not a blob"));
    };
    if !matches!(*mode, "100644" | "100755") || !object_name(object) {
        return Err(String::from("base mutation is not a regular file"));
    }
    checked(git(root).args(["cat-file", "blob", object]).output(), "read base mutation").map(Some)
}

fn object_name(name: &str) -> bool {
    matches!(name.len(), 40 | 64) && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Nonempty summary includes renames/copies, creations/deletions and mode changes.
/// Numstat alone names only the destination of a rename, so those forms stay at core.
fn patch_targets(root: &Path, patch: &[u8]) -> Result<BTreeSet<String>, String> {
    if !parse_patch(root, "--summary", patch)?.is_empty() {
        return Err(String::from("structural mutation patch is not attributed"));
    }
    let numstat = parse_patch(root, "--numstat", patch)?;
    let numstat = std::str::from_utf8(&numstat).map_err(|error| format!("mutation target path: {error}"))?;
    let mut paths = BTreeSet::new();
    for row in numstat.split_terminator('\0') {
        let mut columns = row.splitn(3, '\t');
        let added = columns.next().ok_or("mutation numstat has no added count")?;
        let removed = columns.next().ok_or("mutation numstat has no removed count")?;
        let path = columns.next().ok_or("mutation numstat has no path")?;
        if added.parse::<u64>().is_err() || removed.parse::<u64>().is_err() || !canonical(path) {
            return Err(String::from("mutation numstat is not a canonical text-file path"));
        }
        if path == "Cargo.lock" {
            return Err(String::from("a mutation's lockfile targets are not the real lockfile diff"));
        }
        paths.insert(path.to_owned());
    }
    if paths.is_empty() {
        return Err(String::from("mutation patch names no targets"));
    }
    Ok(paths)
}

fn canonical(path: &str) -> bool {
    !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Drain output while writing stdin, so neither pipe waits for the other's full buffer.
fn parse_patch(root: &Path, format: &str, patch: &[u8]) -> Result<Vec<u8>, String> {
    let mut child = git(root)
        .args(["apply", format, "-z", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("start patch parser: {error}"))?;
    let mut sink = child.stdin.take().ok_or("patch parser has no stdin")?;
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || sink.write_all(patch));
        let output = child.wait_with_output();
        let written = writer.join().map_err(|_panic| String::from("patch input writer panicked"))?;
        let output = checked(output, "parse mutation patch")?;
        written.map_err(|error| format!("write mutation patch: {error}"))?;
        Ok(output)
    })
}
