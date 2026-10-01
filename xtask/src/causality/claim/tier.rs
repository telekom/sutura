//! The kill worktree's own Postgres tier, so a tier-backed claim cell is killed rather than
//! reported `NoTier`.
//!
//! Its OWN and never the root's: a mutated fixture writing state the root's next run reads is the
//! reason the kill worktree was provisioned nothing. The tier keys its data directory and port off
//! the working directory it is started in and publishes `<wt>/.sutura-dev/endpoints.json`, which is
//! exactly where the kill run's discovery walks to - so starting it there is the whole isolation.
//!
//! **Postgres only.** A cell that needs the `clickhouse` tier still ends at its lookup and reads
//! `NoTier`, the requirement being one flag for every tier.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The nix-built tier `nix/postgres-tier.nix` packages, resolved on `PATH`.
pub(super) const POSTGRES: &str = "sutura-postgres-tier";

/// One `NAME=value` the kill run is given.
type Var = (String, String);

/// The only variables a tier's `credentials` may set on the kill run.
const PREFIX: &str = "SUTURA_POSTGRES_TIER_";

/// Why the kill worktree has no tier.
#[derive(Debug)]
pub(super) enum TierError {
    /// The tier program could not be run at all - absent from `PATH`, typically.
    Spawn { step: &'static str, source: io::Error },
    /// It ran and refused; `said` is what it printed.
    Exited { step: &'static str, said: String },
    /// A `credentials` line that is not `export SUTURA_POSTGRES_TIER_<NAME>=<value>`. The line
    /// NUMBER, never its text: it may carry the password.
    Credential { line: usize },
}

impl fmt::Display for TierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { step, source } => write!(f, "could not run `{POSTGRES} {step}`: {source}"),
            Self::Exited { step, said } => write!(f, "`{POSTGRES} {step}` failed: {}", said.trim()),
            Self::Credential { line } => {
                write!(
                    f,
                    "`{POSTGRES} credentials` line {line} is not `export {PREFIX}<NAME>=<value>`"
                )
            }
        }
    }
}

/// A tier started in one kill worktree, stopped when dropped.
pub(super) struct KillTier {
    program: PathBuf,
    wt: PathBuf,
    env: Vec<Var>,
}

impl KillTier {
    /// Start `program`'s tier in `wt` and read what it published.
    ///
    /// The guard exists BEFORE `start` runs, so a start that fails halfway is still stopped.
    pub(super) fn start(program: &Path, wt: &Path) -> Result<Self, TierError> {
        let mut tier = Self {
            program: program.to_path_buf(),
            wt: wt.to_path_buf(),
            env: Vec::new(),
        };
        tier.step("start")?;
        let published = tier.step("credentials")?;
        tier.env = credentials(&published)?;
        Ok(tier)
    }

    /// The credential the kill run needs, overriding the root tier's inherited one.
    pub(super) fn env(&self) -> &[Var] {
        &self.env
    }

    fn step(&self, step: &'static str) -> Result<String, TierError> {
        let out = Command::new(&self.program)
            .current_dir(&self.wt)
            .arg(step)
            .output()
            .map_err(|source| TierError::Spawn { step, source })?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        if out.status.success() {
            Ok(stdout)
        } else {
            let mut said = String::from_utf8_lossy(&out.stderr).into_owned();
            said.push_str(&stdout);
            Err(TierError::Exited { step, said })
        }
    }
}

impl Drop for KillTier {
    fn drop(&mut self) {
        if let Err(why) = self.step("stop") {
            eprintln!("xtask: the kill worktree's Postgres tier did not stop: {why}");
        }
    }
}

/// `export NAME=value` lines, each `NAME` inside the tier's own namespace.
pub(super) fn credentials(published: &str) -> Result<Vec<Var>, TierError> {
    published
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            line.strip_prefix("export ")
                .and_then(|pair| pair.split_once('='))
                .filter(|(name, value)| name.starts_with(PREFIX) && !value.is_empty())
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .ok_or(TierError::Credential { line: index + 1 })
        })
        .collect()
}
