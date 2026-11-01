#![forbid(unsafe_code)]
//! sutura refuses to start when the `BigQuery` driver's debug log is switched on.

#[cfg(test)]
#[cfg(feature = "bigquery")]
mod tests {
    use std::process::{Command, Stdio};

    #[test]
    fn sutura_refuses_to_start_with_the_bigquery_drivers_debug_log_switched_on() {
        let output = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("--version")
            .env("GOOGLE_SDK_GO_LOGGING_LEVEL", "")
            .stdin(Stdio::null())
            .output()
            .expect("the sutura binary runs");
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            !output.status.success(),
            "the process started: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            stderr.contains("GOOGLE_SDK_GO_LOGGING_LEVEL"),
            "the refusal does not name the variable: {stderr}"
        );
    }
}
