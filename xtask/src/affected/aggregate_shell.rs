//! The harness `tests::shell_simulation` and the per-leg `*_aggregate` files run the REAL
//! `ci-aggregate` shell through. Its own file because `affected.rs` sits at the 1000-line cap.

/// The `ci-aggregate` job's `run: |` block, extracted from `.github/workflows/ci.yml` and
/// de-indented, so the simulation exercises the exact script `bash` runs in CI.
fn aggregator_shell() -> String {
    let root = crate::repo::root().expect("the repo root");
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read .github/workflows/ci.yml");
    let lines: Vec<&str> = ci.lines().collect();
    let job = lines
        .iter()
        .position(|l| l.starts_with("  ci-aggregate:"))
        .expect("ci-aggregate job present in ci.yml");
    let run = lines[job..]
        .iter()
        .position(|l| l.trim_start().starts_with("run: |"))
        .map(|i| job + i)
        .expect("the ci-aggregate job has a run: | step");
    let body = &lines[run + 1..];
    let script_indent = body
        .iter()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .expect("the run body is not empty");
    let mut out = Vec::new();
    for line in body {
        if line.trim().is_empty() {
            out.push(String::new());
        } else {
            // A non-blank line shallower than the body is the next step, so it ends the
            // block. The leading bytes are all ASCII spaces (YAML block-scalar indent),
            // so a byte offset is also a char boundary; `split_at` keeps the slice rather
            // than indexing the string.
            let indent = line.len() - line.trim_start().len();
            if indent < script_indent {
                break;
            }
            let (_, rest) = line.split_at(script_indent);
            out.push(rest.to_owned());
        }
    }
    out.join("\n")
}

/// Every category-gated leg's inputs at their unselected value, so a cell names only its own legs.
const DEFAULTS: [(&str, &str); 6] = [
    ("E2E_RESULT", "skipped"),
    ("E2E_REQUIRED", "false"),
    ("ORACLE_RESULT", "skipped"),
    ("ORACLE_SELECTED", "false"),
    ("DH_RESULT", "skipped"),
    ("DH_SELECTED", "false"),
];

/// Run the aggregator shell with the given environment; returns (exit ok, combined output).
pub(super) fn run_aggregator(envs: &[(&str, &str)]) -> (bool, String) {
    let script = aggregator_shell();
    let mut cmd = std::process::Command::new("bash");
    cmd.arg("-c").arg(&script);
    cmd.envs(DEFAULTS);
    cmd.envs(envs.iter().copied());
    let out = cmd.output().expect("bash runs the ci-aggregate shell");
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}
