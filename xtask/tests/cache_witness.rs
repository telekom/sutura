//! The `causality-target-cache` witness, EXECUTED rather than read.
//!
//! `.github/actions/causality-target-cache` refuses a restore that carried nothing: the action
//! reported a hit for the key and `target/causality-target` holds 0 MB, which cannot be a cold
//! start. That shape is exactly what the `path:`/`paths:` defect produced on every run it ever
//! made - `hit on this dependency generation; ...; 0 MB on disk` - with nothing reading it.
//!
//! **A test that grepped the action for its own `exit 1` would prove nothing**, which
//! `xtask/src/workflows/publication.rs` records having cost a witness job: a refusal and prose
//! describing one are the same bytes. So this runs the step's real `run:` body, under the shell the
//! runner uses for `shell: bash`, against a directory this test creates - and the three rows are
//! the three answers that matter: refuse a hit over an empty dir, pass a hit over a full one, pass
//! a MISS over no dir at all.
//!
//! **What it does not hold.** Not that the action's `hit` output is wired to the step's `HIT` env -
//! that is one interpolation in the same file, visible in the diff - and not the runner's own
//! `shell: bash` flag set beyond the `-e -o pipefail` reproduced here. `just lint-actions` reads
//! the same body with `ShellCheck`.

#![cfg(test)]
#![cfg(unix)]

mod tests {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    const ACTION: &str = ".github/actions/causality-target-cache/action.yml";

    /// The directory the witness measures - the one the cache key names.
    const CACHED: &str = "target/causality-target";

    /// The repository root, from this test binary's own manifest directory.
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask/ has a parent")
            .to_path_buf()
    }

    /// The LAST `run: |` block of the action, dedented - the witness step is the last step, and
    /// taking the last one rather than searching for a marker string keeps this test from passing
    /// because it matched a comment.
    fn witness_body() -> String {
        let text = std::fs::read_to_string(root().join(ACTION)).expect("the action is readable");
        let lines: Vec<&str> = text.lines().collect();
        let at = lines
            .iter()
            .rposition(|line| line.trim() == "run: |")
            .expect("the action ends in a `run: |` block");
        let key_column = lines
            .get(at)
            .map(|line| line.len().saturating_sub(line.trim_start().len()))
            .expect("the run line is in range");
        let body: Vec<String> = lines
            .iter()
            .skip(at.saturating_add(1))
            .take_while(|line| line.trim().is_empty() || line.len().saturating_sub(line.trim_start().len()) > key_column)
            .map(|line| line.get(key_column.saturating_add(2)..).unwrap_or("").to_owned())
            .collect();
        assert!(
            body.iter().any(|line| line.contains("exit 1")),
            "the witness body carries no refusal - this test is reading the wrong block"
        );
        body.join("\n")
    }

    /// Run the witness body with `hit` as its `HIT` output, in a fresh tree whose cached directory
    /// holds `megabytes` (or does not exist at all when `None`).
    fn run(tag: &str, hit: &str, megabytes: Option<usize>) -> Output {
        let tree = std::env::temp_dir().join(format!("sutura-witness-{}-{tag}", std::process::id()));
        if tree.exists() {
            std::fs::remove_dir_all(&tree).expect("a leftover temp tree is removable");
        }
        std::fs::create_dir_all(&tree).expect("temp tree");
        if let Some(megabytes) = megabytes {
            std::fs::create_dir_all(tree.join(CACHED)).expect("the cached dir");
            std::fs::write(
                tree.join(CACHED).join("artifact"),
                vec![b'x'; megabytes.saturating_mul(0x0010_0000)],
            )
            .expect("fill the cached dir");
        }
        let script = tree.join("witness.sh");
        std::fs::write(&script, witness_body()).expect("write the witness body");
        // The runner's own invocation for `shell: bash`, which is what makes the `mb=0` guard
        // above the refusal load-bearing rather than decorative.
        let output = Command::new("bash")
            .args(["--noprofile", "--norc", "-e", "-o", "pipefail"])
            .arg(&script)
            .current_dir(&tree)
            .env("SCOPE", "ci")
            .env("HIT", hit)
            .env("HIT_PRIMARY", hit)
            .env("RESTORED", "causality-target-v2-Linux-X64-ci-abc-def")
            .env("WRITES", "false")
            .env("GITHUB_STEP_SUMMARY", tree.join("summary.md"))
            .output()
            .expect("bash runs");
        std::fs::remove_dir_all(&tree).expect("the temp tree this test created is removable");
        output
    }

    /// THE REFUSAL, which is the whole point: an entry restored for this key over an empty
    /// directory. Both spellings of empty - absent, and present with nothing in it - because the
    /// defect produced the first and a broken save would produce the second.
    ///
    /// The "present with nothing in it" leg is what caught the `du -sm` block-rounding defect on
    /// CI: an empty directory (holding only a 0-byte file) owns one allocation block on the runner
    /// filesystem, so a block-based `du -sm` read "1 MB" and the refusal - which needed 0 - never
    /// fired even though nothing was carried. The refusal is on FILE CONTENT BYTES (`find` a
    /// regular file larger than 0 bytes), the identical semantic on every filesystem, so this leg
    /// refuses on macOS's APFS as much as on an ext4 runner.
    #[test]
    fn a_hit_that_carried_no_bytes_is_refused() {
        for (tag, megabytes) in [("absent", None), ("empty", Some(0))] {
            let output = run(tag, "true", megabytes);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(
                output.status.code(),
                Some(1),
                "a hit carrying nothing must exit 1 ({tag}): {stderr}"
            );
            assert!(stderr.contains("RED"), "{tag}: {stderr}");
            assert!(stderr.contains("holds no non-empty file"), "{tag}: {stderr}");
        }
    }

    /// AND IT DOES NOT REDDEN CORRECT WORK, which is the half that decides whether a gate survives:
    /// a hit over a directory that actually holds bytes is the state this cache exists to produce.
    #[test]
    fn a_hit_that_carried_bytes_passes() {
        let output = run("full", "true", Some(2));
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(0),
            "a hit over a full dir must pass: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!stdout.contains("0 MB on disk"), "the size is reported non-zero: {stdout}");
    }

    /// A MISS IS LEGITIMATE on the first run of a dependency generation, and a docs-only push to
    /// `main` never creates the directory at all. Reddening either would be inverted logic.
    #[test]
    fn a_miss_over_no_directory_passes() {
        let output = run("miss", "false", None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.code(),
            Some(0),
            "a miss must pass: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains("MISS"), "{stdout}");
        assert!(stdout.contains("0 MB on disk"), "{stdout}");
    }
}
