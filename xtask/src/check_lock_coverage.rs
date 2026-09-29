/// Check that every tracked Cargo.lock is covered by a dependabot.yml directories entry.
use crate::Verdict;

pub(crate) fn run(_args: &[String]) -> Verdict {
    match gather_locks() {
        Ok(locks) => check_coverage(&locks),
        Err(e) => {
            eprintln!("check-lock-coverage: {e}");
            Verdict::Fail
        }
    }
}

fn gather_locks() -> Result<Vec<String>, String> {
    let out = std::process::Command::new("git")
        .args(["ls-files", "*Cargo.lock"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8(out.stdout)
        .map_err(|e| format!("UTF-8: {e}"))?
        .lines()
        .map(|line| {
            std::path::Path::new(line)
                .parent()
                .filter(|x| !x.as_os_str().is_empty())
                .map_or_else(|| "/".into(), |x| format!("/{}", x.display()))
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn check_coverage(locks: &[String]) -> Verdict {
    let yaml = match std::fs::read_to_string(".github/dependabot.yml") {
        Ok(y) => y,
        Err(e) => {
            eprintln!("check-lock-coverage: yaml: {e}");
            return Verdict::Fail;
        }
    };
    let dirs = match parse_dirs(&yaml) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("check-lock-coverage: {e}");
            return Verdict::Fail;
        }
    };
    let uncovered: Vec<_> = locks.iter().filter(|l| !dirs.contains(l)).collect();
    if uncovered.is_empty() {
        println!("check-lock-coverage: ok - {} lock(s)", locks.len());
        Verdict::Pass
    } else {
        eprintln!("check-lock-coverage: {} uncovered:", uncovered.len());
        for l in uncovered {
            eprintln!("  {l}");
        }
        Verdict::Fail
    }
}

fn parse_dirs(yaml: &str) -> Result<Vec<String>, String> {
    let mut dirs = Vec::new();
    let mut in_cargo = false;
    for line in yaml.lines() {
        if line.contains("package-ecosystem:") && line.contains("cargo") {
            in_cargo = true;
        } else if in_cargo && line.trim_start().starts_with("- package-ecosystem:") {
            break;
        } else if in_cargo && let Some(s) = line.trim_start().strip_prefix("directories:") {
            let inner = s
                .trim()
                .strip_prefix('[')
                .and_then(|r| r.strip_suffix(']'))
                .ok_or_else(|| "malformed".to_owned())?;
            for item in inner.split(',') {
                let d = item.trim().trim_matches('"').trim_matches('\'');
                if !d.is_empty() {
                    dirs.push(d.to_owned());
                }
            }
        }
    }
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncovered() {
        let yaml = "updates:\n  - package-ecosystem: cargo\n    directories: [\"/\"]\n";
        assert!(!parse_dirs(yaml).unwrap().contains(&"/fuzz".to_owned()));
    }
    #[test]
    fn covered() {
        let yaml = "updates:\n  - package-ecosystem: cargo\n    directories: [\"/\", \"/fuzz\"]\n";
        assert_eq!(parse_dirs(yaml).unwrap().len(), 2);
    }
}
