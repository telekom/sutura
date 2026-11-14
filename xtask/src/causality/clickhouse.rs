//! The mounted driver is built outside Cargo, so Cargo's isolation cannot refresh it.
//! Rebuild the existing host package from the nextest command's own tree before each run.
//! This rebinds the dynamic library only; it does not rebuild a driver's linked archive.

use std::path::Path;
use std::process::{Command, Output};

const DRIVER_ENV: &str = "SUTURA_CLICKHOUSE_ADBC_DRIVER";

/// Bind the driver built from the exact working tree nextest is about to measure.
pub(super) fn bind(command: &mut Command) -> Result<(), String> {
    let output = {
        let dir = command.get_current_dir().ok_or("the test command has no working tree")?;
        // Synthetic Cargo-only repositories carry a root flake but no xtask package. A missing
        // driver input in this workspace must reach Nix and fail preparation.
        if !dir.join("xtask/Cargo.toml").is_file() {
            return Ok(());
        }
        println!("  mounted ClickHouse driver source: {}", dir.display());
        build(dir)
            .output()
            .map_err(|why| format!("could not build the mounted ClickHouse driver: {why}"))?
    };
    bind_built(command, &output)
}

fn build(dir: &Path) -> Command {
    let mut command = Command::new("nix");
    crate::repo::strip_git_env(&mut command);
    command
        .current_dir(dir)
        .args(["build", "--no-link", "--print-out-paths", ".#adbc-driver-clickhouse-host"]);
    command
}

fn bind_built(command: &mut Command, output: &Output) -> Result<(), String> {
    if !output.status.success() {
        return Err(format!(
            "the mounted ClickHouse driver build failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let text = std::str::from_utf8(&output.stdout).map_err(|why| format!("the driver output is not UTF-8: {why}"))?;
    let mut paths = text.lines();
    let Some(root) = paths.next().filter(|path| !path.is_empty()) else {
        return Err(String::from("the driver build returned no output path"));
    };
    if paths.next().is_some() || !Path::new(root).is_absolute() {
        return Err(String::from("the driver build must return one absolute output path"));
    }
    let library = Path::new(root)
        .join("lib")
        .join(format!("libadbc_driver_clickhouse{}", std::env::consts::DLL_SUFFIX));
    if !library.is_file() {
        return Err(format!("the driver build returned no mounted library: {}", library.display()));
    }
    println!("  mounted ClickHouse driver: {}", library.display());
    command.env(DRIVER_ENV, library);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::{DRIVER_ENV, bind};

    #[test]
    fn a_root_missing_its_driver_input_refuses_the_inherited_mount() {
        let root = std::env::temp_dir().join(format!("sutura-causality-missing-driver-{}", std::process::id()));
        std::fs::create_dir_all(root.join("xtask")).expect("a root fixture");
        std::fs::write(root.join("xtask/Cargo.toml"), "[package]\nname = \"xtask\"\n").expect("an independent workspace marker");
        std::fs::write(root.join("flake.nix"), "{ outputs = _: import ./nix/clickhouse-adbc.nix; }\n")
            .expect("a flake with an absent driver input and no fetched inputs");
        let mut command = Command::new("cargo");
        command.current_dir(&root).env(DRIVER_ENV, "/stale/driver.so");
        let prepared = bind(&mut command);
        std::fs::remove_dir_all(root).expect("the root fixture is removed");
        assert!(
            prepared.is_err(),
            "a missing driver input must not preserve a successful inherited mount"
        );
    }
}
