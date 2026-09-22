#![forbid(unsafe_code)]
//! The jscpd config is one strict ignore-only document consumed by both gate paths.

#![cfg(test)]
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::{Command, Output};

// The fixture tree is a [`scratch_tree::Tree`] - the sweep-before-create and the `Drop` sweep the
// module holds are the two halves #938's leftover trap had - rather than a hand-rolled exclusive
// `create_dir` on a pid-and-counter-keyed path, which is what this file did until #938. `seal`
// and `Fixture` are the module's own tests' items; this integration test does not use them, so
// the include expects `dead_code` on it rather than weakening the module for everyone.
#[path = "scratch_tree/mod.rs"]
#[expect(
    dead_code,
    reason = "the module's own tests use `seal` and `Fixture`; this integration test does not"
)]
#[cfg(test)]
mod scratch_tree;

use scratch_tree::Tree;

// The tag must stay unique PER CALL, not per process: nextest runs this file's tests
// concurrently, and `Tree::of` sweeps before it creates, so two live trees sharing a tag would
// delete each other mid-test. A per-call counter restores the distinctness the pid-and-counter
// fixture had, without the exclusive create that made a recycled pid fail its NEXT run (#938).
static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tag(prefix: &str) -> String {
    format!("{prefix}-{}", CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

const CONFIG: &str = "{ \"ignore\": [] }\n";

const JSCPD: &str = r#"
set -eu
printf '%s\n' "$@" > "$SUTURA_JSCPD_LEDGER"
config=
output=
take=
for arg in "$@"; do
  case "$take" in
    config) config="$arg"; take= ;;
    output) output="$arg"; take= ;;
    *)
      case "$arg" in
        --config) take=config ;;
        --output) take=output ;;
      esac
      ;;
  esac
done
[ -n "$config" ]
[ -n "$output" ]
cp "$config" "$SUTURA_JSCPD_LEDGER.before"
if [ "${SUTURA_JSCPD_MUTATE:-no}" = yes ]; then
  printf '{"ignore":["changed-after-launch/**"]}\n' > "$SUTURA_JSCPD_SOURCE"
fi
cp "$config" "$SUTURA_JSCPD_LEDGER.after"
mkdir -p "$output"
if [ "${SUTURA_JSCPD_REPORT:-yes}" = yes ]; then
  printf '{"duplicates":[]}' > "$output/jscpd-report.json"
fi
"#;

const NIX: &str = r#"
set -eu
printf '%s\n' "$@" > "$SUTURA_NIX_LEDGER"
"#;

#[derive(Debug)]
struct Observed {
    output: Output,
    args: Option<Vec<String>>,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    source: Option<Vec<u8>>,
}

fn executable(path: &Path, body: &str) {
    std::fs::write(path, format!("#!/bin/sh\n{body}")).expect("write fake executable");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("make fake executable runnable");
}

fn fixture_path(bin: &Path) -> OsString {
    let mut paths = vec![bin.to_owned()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").expect("test PATH")));
    std::env::join_paths(paths).expect("fixture PATH")
}

fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok()
}

fn observe(config: Option<&[u8]>, mutate: bool, report: bool) -> Observed {
    let tree = Tree::of(
        &tag("jscpd-config-xtask"),
        &[
            ("Cargo.toml", b"[workspace]\n" as &[u8]),
            ("flake.nix", b"{}\n"),
            ("in_scope.rs", b"fn in_scope() {}\n"),
            ("devco/dup-ignore", b"# none\n"),
            ("nix/run-gate.sh", include_bytes!("../../nix/run-gate.sh")),
        ],
    );
    let root = tree.root().to_path_buf();
    let bin = root.join("bin");
    let devco = root.join("devco");
    std::fs::create_dir_all(&bin).expect("fake PATH");
    let source = devco.join("jscpd.json");
    if let Some(bytes) = config {
        std::fs::write(&source, bytes).expect("jscpd config");
    }
    executable(&bin.join("jscpd"), JSCPD);
    let ledger = root.join("jscpd.args");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("check-jscpd")
        .current_dir(&root)
        .env("PATH", fixture_path(&bin))
        .env("SUTURA_JSCPD_LEDGER", &ledger)
        .env("SUTURA_JSCPD_SOURCE", &source)
        .env("SUTURA_JSCPD_MUTATE", if mutate { "yes" } else { "no" })
        .env("SUTURA_JSCPD_REPORT", if report { "yes" } else { "no" })
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("BASH_ENV")
        .output()
        .expect("execute the real xtask gate");
    let args = std::fs::read_to_string(&ledger)
        .ok()
        .map(|text| text.lines().map(String::from).collect());
    let observed = Observed {
        output,
        args,
        before: read(&ledger.with_extension("args.before")),
        after: read(&ledger.with_extension("args.after")),
        source: read(&source),
    };
    drop(tree);
    observed
}

#[test]
fn xtask_consumes_one_immutable_config_snapshot_and_its_report() {
    let observed = observe(Some(CONFIG.as_bytes()), true, true);
    assert_eq!(observed.output.status.code(), Some(0), "{observed:?}");
    let args = observed.args.as_ref().expect("jscpd must run");
    assert_eq!(
        args.iter().filter(|arg| arg.as_str() == "--config").count(),
        1,
        "{observed:?}"
    );
    assert!(!args.iter().any(|arg| arg == "--ignore"), "{observed:?}");
    assert_eq!(observed.before.as_deref(), Some(CONFIG.as_bytes()), "{observed:?}");
    assert_eq!(
        observed.after, observed.before,
        "the child must keep reading the snapshot: {observed:?}"
    );
    assert_ne!(
        observed.source, observed.before,
        "the fake must mutate the source after launch"
    );
}

#[test]
fn xtask_refuses_a_successful_child_that_produced_no_report() {
    let observed = observe(Some(CONFIG.as_bytes()), false, false);
    assert_eq!(observed.output.status.code(), Some(1), "{observed:?}");
    assert!(observed.args.is_some(), "the fake child must have run: {observed:?}");
}

#[test]
fn xtask_refuses_missing_malformed_unknown_and_wrong_type_configs_before_launch() {
    let cases = [
        ("missing", None),
        ("malformed", Some(b"{" as &[u8])),
        ("unknown", Some(br#"{"ignore":[],"format":["rust"]}"# as &[u8])),
        ("wrong-type", Some(br#"{"ignore":"target/**"}"# as &[u8])),
    ];
    for (name, config) in cases {
        let observed = observe(config, false, true);
        assert_eq!(observed.output.status.code(), Some(1), "{name}: {observed:?}");
        assert!(
            observed.args.is_none(),
            "{name}: jscpd ran before config refusal: {observed:?}"
        );
    }
}

#[test]
fn run_gate_passes_the_same_config_to_the_pinned_jscpd_route() {
    let tree = Tree::of(
        &tag("jscpd-config-run-gate"),
        &[
            ("devco/jscpd.json", CONFIG.as_bytes()),
            ("nix/run-gate.sh", include_bytes!("../../nix/run-gate.sh")),
        ],
    );
    let root = tree.root().to_path_buf();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("fake PATH");
    executable(&bin.join("nix"), NIX);
    let ledger = root.join("nix.args");
    let bash = std::env::split_paths(&std::env::var_os("PATH").expect("test PATH"))
        .map(|path| path.join("bash"))
        .find(|path| path.is_file())
        .expect("bash on the test PATH");
    let output = Command::new(bash)
        .args(["--noprofile", "--norc", "nix/run-gate.sh", "jscpd"])
        .current_dir(&root)
        .env("PATH", &bin)
        .env("SUTURA_NIX_LEDGER", &ledger)
        .env_remove("BASH_ENV")
        .output()
        .expect("execute the real run-gate jscpd arm");
    let args: Vec<String> = std::fs::read_to_string(&ledger)
        .expect("nix invocation ledger")
        .lines()
        .map(String::from)
        .collect();
    drop(tree);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(args.iter().filter(|arg| arg.as_str() == "--config").count(), 1, "{args:?}");
    assert!(!args.iter().any(|arg| arg == "--ignore"), "{args:?}");
    let position = args.iter().position(|arg| arg == "--config").expect("config argument");
    assert_eq!(
        args.get(position + 1).map(String::as_str),
        Some("devco/jscpd.json"),
        "{args:?}"
    );
}
