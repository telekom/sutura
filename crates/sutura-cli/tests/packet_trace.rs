#![forbid(unsafe_code)]
//! sutura refuses to start when the Oracle driver's packet trace is switched on.

#[cfg(test)]
#[cfg(feature = "oracle")]
mod tests {
    use std::process::{Command, Stdio};

    #[test]
    fn sutura_refuses_to_start_with_the_oracle_drivers_packet_trace_switched_on() {
        let output = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("--version")
            .env("RSO_DEBUG_PACKETS", "")
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
            stderr.contains("RSO_DEBUG_PACKETS"),
            "the refusal does not name the variable: {stderr}"
        );
    }
}
