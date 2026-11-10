#![forbid(unsafe_code)]
//! The release monitor executes against a fake forge CLI, including failed and absent releases.

#![cfg(test)]

use std::path::Path;
use std::process::Command;

#[test]
fn a_release_status_requires_successful_tag_workflows_and_a_published_release() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root");
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("release-status-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("fake tools directory");
    let bash = std::env::split_paths(&std::env::var_os("PATH").expect("tool path"))
        .map(|dir| dir.join("bash"))
        .find(|path| path.is_file())
        .expect("bash on tool path");
    std::fs::write(
        root.join("gh"),
        format!(
            "#!{}\n{}",
            bash.display(),
            r#"set -eu
case "$1 $2" in
  'run list')
    [ "$RELEASE_CASE" != absent ] || exit 0
    if [ "$4" = fuzz.yml ]; then echo 456; else echo 123; fi ;;
  'run watch')
    [ "$RELEASE_CASE" != failed ] || exit 1
    [ "$RELEASE_CASE $3" != 'fuzz-failed 456' ] ;;
  'run view') echo v0.12.1 ;;
  'release view')
    [ "$RELEASE_CASE" != missing ] || exit 1
    if [ "$RELEASE_CASE" = draft ]; then echo true; else echo false; fi ;;
  *) exit 90 ;;
esac
"#,
        ),
    )
    .expect("fake forge CLI");
    std::fs::write(root.join("sleep"), format!("#!{}\nexit 0\n", bash.display())).expect("fake wait");
    let chmod = Command::new("chmod")
        .arg("+x")
        .arg(root.join("gh"))
        .arg(root.join("sleep"))
        .status()
        .expect("make fake tools executable");
    assert!(chmod.success());
    let path = format!("{}:{}", root.display(), std::env::var("PATH").expect("tool path"));
    for case in ["published", "failed", "fuzz-failed", "missing", "draft", "absent"] {
        let result = Command::new("bash")
            .arg(repo.join("nix/wait-for-release.sh"))
            .env("PATH", &path)
            .env("RELEASE_SHA", "release-commit")
            .env("RELEASE_CASE", case)
            .output()
            .expect("execute the actual release monitor");
        assert_eq!(
            result.status.success(),
            case == "published",
            "{case}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    std::fs::remove_dir_all(root).expect("remove fake tools");
}
