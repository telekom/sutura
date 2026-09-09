//! The jscpd config is one strict ignore-only document consumed by both gate paths.

#![cfg(test)]
#![cfg(unix)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const CONFIG: &str = r#"{
  "ignore": [
    "target/**",
    "site/**",
    "result/**",
    "result-*/**",
    ".pixi/**",
    ".sutura-dev/**",
    "report/**",
    "**/.prek-cache/**"
  ]
}
"#;

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

#[expect(
    clippy::create_dir,
    reason = "the PID-and-counter fixture must be newly allocated, never reused"
)]
fn fixture(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "sutura-jscpd-config-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).expect("create a distinct fixture");
    root
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
    let root = fixture("xtask");
    let bin = root.join("bin");
    let devco = root.join("devco");
    let nix = root.join("nix");
    std::fs::create_dir_all(&bin).expect("fake PATH");
    std::fs::create_dir_all(&devco).expect("config directory");
    std::fs::create_dir_all(&nix).expect("run-gate directory");
    std::fs::write(root.join("Cargo.toml"), "[workspace]\n").expect("workspace marker");
    std::fs::write(root.join("flake.nix"), "{}\n").expect("Nix marker");
    std::fs::write(root.join("in_scope.rs"), "fn in_scope() {}\n").expect("Rust census subject");
    std::fs::write(devco.join("dup-ignore"), "# none\n").expect("empty duplicate allowlist");
    std::fs::write(nix.join("run-gate.sh"), include_bytes!("../../nix/run-gate.sh")).expect("real tier source");
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
    std::fs::remove_dir_all(root).expect("remove the owned fixture");
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
    let root = fixture("run-gate");
    let bin = root.join("bin");
    let devco = root.join("devco");
    let nix_dir = root.join("nix");
    std::fs::create_dir_all(&bin).expect("fake PATH");
    std::fs::create_dir_all(&devco).expect("config directory");
    std::fs::create_dir_all(&nix_dir).expect("run-gate directory");
    std::fs::write(devco.join("jscpd.json"), CONFIG).expect("shared config");
    let run_gate = nix_dir.join("run-gate.sh");
    std::fs::write(&run_gate, include_bytes!("../../nix/run-gate.sh")).expect("real run-gate script");
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
    std::fs::remove_dir_all(root).expect("remove the owned fixture");
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
