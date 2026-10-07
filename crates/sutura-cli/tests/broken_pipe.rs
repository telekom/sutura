#![forbid(unsafe_code)]
//! `sutura catalog | head` must end quietly: a reader that closes the pipe early is not an error,
//! and a print that fails for any other reason still is.

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::io::Read as _;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    fn example_catalog() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player/catalog")
    }

    #[test]
    fn a_reader_closing_stdout_early_ends_the_command_without_a_panic() {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("catalog")
            .arg(example_catalog())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the sutura binary runs");
        drop(child.stdout.take());
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .expect("stderr is piped")
            .read_to_string(&mut stderr)
            .expect("stderr is readable");
        let status = child.wait().expect("the child exits");

        assert!(!stderr.contains("panicked"), "a closed pipe panicked: {stderr}");
        assert!(status.success(), "a closed pipe failed the command: {status}\n{stderr}");
    }

    #[test]
    fn a_print_failing_for_another_reason_than_a_closed_pipe_still_panics() {
        // Standard output is a regular file under a zero size limit with `SIGXFSZ` ignored, so the
        // write fails with "File too large" rather than "Broken pipe". Both the limit and the
        // ignored signal survive the `exec`. `/bin/sh` rather than `/dev/full`, which only Linux has.
        let out = std::env::temp_dir().join(format!("sutura-print-failure-{}.out", std::process::id()));
        let output = Command::new("/bin/sh")
            .args(["-c", "trap '' XFSZ; ulimit -f 0; exec \"$0\" \"$@\" > \"$OUT\""])
            .arg(env!("CARGO_BIN_EXE_sutura"))
            .arg("catalog")
            .arg(example_catalog())
            .env("OUT", &out)
            .stdin(Stdio::null())
            .output()
            .expect("the shell runs the sutura binary");
        drop(std::fs::remove_file(&out));
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            stderr.contains("failed printing to stdout"),
            "a print that failed for another reason was swallowed: {stderr}"
        );
        assert!(
            !output.status.success(),
            "a print that failed for another reason ended the command cleanly: {}",
            output.status
        );
    }

    #[test]
    fn serve_whose_standard_output_is_gone_fails_instead_of_stopping_cleanly() {
        // The shell holds `serve` back until this test has dropped the read end of its standard
        // output, so the banner meets a closed pipe on every run rather than on most of them. It
        // reads one line from standard input, and the drop of that handle is the end of the input.
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read _; exec \"$0\" \"$@\""])
            .arg(env!("CARGO_BIN_EXE_sutura"))
            .arg("serve")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the shell runs the sutura binary");
        drop(child.stdout.take());
        drop(child.stdin.take());
        let output = child.wait_with_output().expect("the child exits");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            stderr.contains("failed printing to stdout"),
            "serve did not report that it could not print: {stderr}"
        );
        assert!(
            !output.status.success(),
            "serve ended cleanly with nothing to print to: {}",
            output.status
        );
    }
}
