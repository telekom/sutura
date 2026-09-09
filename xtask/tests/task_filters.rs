//! Literal selector parity through the real `check-scope` entry point, without running a venue.
//! These fixtures hold text selection only, not shell/Nix evaluation or live identity behavior.

#![cfg(test)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const JUST: &str = "\
test:
    cargo nextest run --workspace --all-features
bigquery-acceptance:
    echo 'scope: sutura-exec-bigquery; use `just test` for the workspace'
    cargo nextest run -p sutura-exec-bigquery --all-features --run-ignored only \\
      -E 'binary(acceptance)'
";

const FLAKE: &str = "\
{
  apps.bigquery-acceptance = {
    type = \"app\";
    program = builtins.toString (pkgs.writeShellScript \"acceptance\" ''
      exec cargo nextest run --cargo-profile ci -p sutura-exec-bigquery --all-features \\
        --run-ignored only -E 'binary(acceptance)' \"$@\"
    '');
  };
}
";

#[expect(
    clippy::create_dir,
    reason = "the PID-and-counter fixture must be newly allocated, never reused"
)]
fn run(just: &str, flake: &[u8]) -> Output {
    let root = std::env::temp_dir().join(format!(
        "sutura-task-filters-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).expect("create a distinct fixture");
    let result = (|| {
        std::fs::write(root.join("Cargo.toml"), "[workspace]\n")?;
        std::fs::write(root.join("justfile"), just)?;
        std::fs::write(root.join("flake.nix"), flake)?;
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check-scope")
            .current_dir(&root)
            .env_clear()
            .output()
    })();
    std::fs::remove_dir_all(&root).expect("remove only the owned fixture before asserting");
    result.expect("run the real gate without a shell or venue command")
}

#[test]
fn the_scope_gate_refuses_a_same_named_app_with_a_different_selector() {
    let control = run(JUST, FLAKE.as_bytes());
    let changed = FLAKE.replace("binary(acceptance)", "binary(exchanged_identity)");
    let refused = run(JUST, changed.as_bytes());
    assert_eq!(control.status.code(), Some(0), "{control:?}");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let message = String::from_utf8(refused.stderr).expect("text diagnostic");
    assert!(message.contains("bigquery-acceptance"), "{message}");
    assert!(message.contains("filter differs"), "{message}");
}

#[test]
fn the_scope_gate_compares_literals_not_comments_or_unrelated_flags() {
    let just = JUST.replace(
        "-E 'binary(acceptance)'",
        "--no-capture -E \"binary(acceptance)\" # -E 'not a selector'",
    );
    let flake = FLAKE.replace(
        "      exec cargo nextest",
        "      # cargo nextest run -E 'not a selector'\n      echo \"cargo nextest run -E 'not a command'\"\n      exec cargo nextest",
    );
    let output = run(&just, flake.as_bytes());
    let changed = just.replace("binary(acceptance)", "binary(other)");
    let refused = run(&changed, flake.as_bytes());
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
}

#[test]
fn the_scope_gate_refuses_incomplete_or_unreadable_literal_pairs() {
    let cases = [
        (
            "missing recipe selector",
            JUST.replace("-E 'binary(acceptance)'", ""),
            String::from(FLAKE),
        ),
        (
            "missing app selector",
            String::from(JUST),
            FLAKE.replace("-E 'binary(acceptance)'", ""),
        ),
        (
            "both selectors missing",
            JUST.replace("-E 'binary(acceptance)'", ""),
            FLAKE.replace("-E 'binary(acceptance)'", ""),
        ),
        ("missing app", String::from(JUST), String::from("{}\n")),
        (
            "missing recipe",
            JUST.replace("bigquery-acceptance:", "other:"),
            String::from(FLAKE),
        ),
        ("duplicate recipe", format!("{JUST}{JUST}"), String::from(FLAKE)),
        ("duplicate app", String::from(JUST), format!("{FLAKE}{FLAKE}")),
        (
            "unquoted selector",
            String::from(JUST),
            FLAKE.replace("'binary(acceptance)'", "binary(acceptance)"),
        ),
        (
            "unclosed selector",
            String::from(JUST),
            FLAKE.replace("'binary(acceptance)'", "'binary(acceptance)"),
        ),
        (
            "empty selector",
            String::from(JUST),
            FLAKE.replace("'binary(acceptance)'", "''"),
        ),
        (
            "expanded selector",
            JUST.replace("'binary(acceptance)'", "\"$FILTER\""),
            String::from(FLAKE),
        ),
        (
            "duplicate selector",
            JUST.replace("-E 'binary(acceptance)'", "-E 'binary(acceptance)' -E 'binary(other)'"),
            String::from(FLAKE),
        ),
        (
            "embedded hash before duplicate selector",
            JUST.replace(
                "-E 'binary(acceptance)'",
                "-E 'binary(acceptance)' name#suffix -E 'binary(other)'",
            ),
            String::from(FLAKE),
        ),
        ("unclosed app", String::from(JUST), FLAKE.replace("  };\n}", "")),
    ];
    let outcomes: Vec<_> = cases
        .iter()
        .map(|(name, just, flake)| (*name, run(just, flake.as_bytes())))
        .collect();
    let unreadable = run(JUST, &[0xff]);
    for (name, output) in outcomes {
        assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
        let message = String::from_utf8(output.stderr).expect("text diagnostic");
        assert!(message.contains("filter"), "{name}: {message}");
    }
    assert_eq!(unreadable.status.code(), Some(1), "{unreadable:?}");
    let message = String::from_utf8(unreadable.stderr).expect("text diagnostic");
    assert!(message.contains("could not read flake.nix"), "{message}");
}
