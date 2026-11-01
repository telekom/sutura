#![forbid(unsafe_code)]
//! `sutura-operator --version`, the command the release smoke-tests in every operator image.

#[cfg(test)]
mod tests {
    use std::process::Command;

    #[test]
    fn version_prints_the_binary_and_its_package_version() {
        let output = Command::new(env!("CARGO_BIN_EXE_sutura-operator"))
            .arg("--version")
            .output()
            .expect("the sutura-operator binary runs");

        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            format!("sutura-operator {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}
