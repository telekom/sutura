#![forbid(unsafe_code)]
//! `sutura catalog | head` must end quietly: a reader that closes the pipe early is not an error.

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::io::Read as _;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};

    #[test]
    fn a_reader_closing_stdout_early_ends_the_command_without_a_panic() {
        let catalog = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/single-player/catalog");
        let mut child = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("catalog")
            .arg(catalog)
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
}
