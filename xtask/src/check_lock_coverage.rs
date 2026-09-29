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
    let locks = scan_locks()?;

    Ok(locks
        .iter()
        .map(|line| {
            std::path::Path::new(line)
                .parent()
                .filter(|x| !x.as_os_str().is_empty())
                .map_or_else(
                    || "/".into(),
                    |x| {
                        let parent_str = x.display().to_string();
                        if parent_str == "." {
                            "/".into()
                        } else if let Some(stripped) = parent_str.strip_prefix("./") {
                            format!("/{stripped}")
                        } else {
                            format!("/{parent_str}")
                        }
                    },
                )
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect())
}

fn scan_locks() -> Result<Vec<String>, String> {
    let mut locks = Vec::new();
    scan_dir(".", &mut locks)?;
    Ok(locks)
}

fn scan_dir(dir: &str, locks: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir}: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let path_str = path.to_string_lossy().to_string();
        if path.file_name().is_some_and(|n| n == "Cargo.lock") {
            locks.push(path_str);
        } else if path.is_dir() && !is_ignored_dir(&path) && !is_symlink(&path)? {
            scan_dir(&path_str, locks)?;
        }
    }
    Ok(())
}

fn is_ignored_dir(path: &std::path::Path) -> bool {
    path.file_name().is_some_and(|n| {
        let name = n.to_str().unwrap_or("");
        name == "target" || name == "node_modules" || name == ".git"
    })
}

fn is_symlink(path: &std::path::Path) -> Result<bool, String> {
    path.symlink_metadata()
        .map(|m| m.file_type().is_symlink())
        .map_err(|e| e.to_string())
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

    #[test]
    fn walk_finds_missing_directory_uncovered() {
        let td = std::env::temp_dir().join(format!(
            "sutura-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());
        std::fs::create_dir_all(&td).unwrap();
        std::fs::write(td.join("Cargo.lock"), "").unwrap();
        std::fs::create_dir_all(td.join("sub")).unwrap();
        std::fs::write(td.join("sub/Cargo.lock"), "").unwrap();

        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&td).unwrap();
        let result = scan_locks().unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());

        assert_eq!(result.len(), 2);
        let normalized: Vec<_> = result
            .iter()
            .map(|p| {
                std::path::Path::new(p)
                    .parent()
                    .filter(|x| !x.as_os_str().is_empty())
                    .map_or_else(
                        || "/".into(),
                        |x| {
                            let parent_str = x.display().to_string();
                            if parent_str == "." {
                                "/".into()
                            } else if let Some(stripped) = parent_str.strip_prefix("./") {
                                format!("/{stripped}")
                            } else {
                                format!("/{parent_str}")
                            }
                        },
                    )
            })
            .collect();
        assert!(normalized.contains(&"/".to_owned()));
        assert!(normalized.contains(&"/sub".to_owned()));
    }

    #[test]
    fn walk_respects_ignored_directories() {
        let td = std::env::temp_dir().join(format!(
            "sutura-test-ignored-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());
        std::fs::create_dir_all(&td).unwrap();
        std::fs::write(td.join("Cargo.lock"), "").unwrap();

        // Create directories that should be ignored
        std::fs::create_dir_all(td.join(".git")).unwrap();
        std::fs::write(td.join(".git/Cargo.lock"), "").unwrap();
        std::fs::create_dir_all(td.join("target")).unwrap();
        std::fs::write(td.join("target/Cargo.lock"), "").unwrap();
        std::fs::create_dir_all(td.join("node_modules")).unwrap();
        std::fs::write(td.join("node_modules/Cargo.lock"), "").unwrap();

        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&td).unwrap();
        let result = scan_locks().unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());

        // Should only find the root Cargo.lock
        assert_eq!(result.len(), 1);
        assert!(result[0].ends_with("Cargo.lock"));
    }

    #[test]
    fn walk_finds_lock_in_dot_directories_except_git() {
        let td = std::env::temp_dir().join(format!(
            "sutura-test-dotdir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());
        std::fs::create_dir_all(&td).unwrap();
        std::fs::write(td.join("Cargo.lock"), "").unwrap();

        // .git should be ignored
        std::fs::create_dir_all(td.join(".git")).unwrap();
        std::fs::write(td.join(".git/Cargo.lock"), "").unwrap();

        // But other dot-dirs should be included
        std::fs::create_dir_all(td.join(".config")).unwrap();
        std::fs::write(td.join(".config/Cargo.lock"), "").unwrap();

        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&td).unwrap();
        let result = scan_locks().unwrap();
        std::env::set_current_dir(orig_dir).unwrap();
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());

        // Should find root and .config locks, but not .git
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn walk_does_not_follow_symlinks() {
        let td = std::env::temp_dir().join(format!(
            "sutura-test-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());
        std::fs::create_dir_all(&td).unwrap();
        std::fs::write(td.join("Cargo.lock"), "").unwrap();

        // Create a real directory with a lock
        std::fs::create_dir_all(td.join("real")).unwrap();
        std::fs::write(td.join("real/Cargo.lock"), "").unwrap();

        // Create a symlink to a directory that has a lock
        // We use a temporary directory to be the target
        let target_dir = std::env::temp_dir().join(format!(
            "sutura-symlink-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _: () = std::fs::remove_dir_all(&target_dir).ok().unwrap_or(());
        std::fs::create_dir_all(&target_dir).unwrap();
        std::fs::write(target_dir.join("Cargo.lock"), "").unwrap();

        // Create the symlink
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_dir, td.join("symlinked")).unwrap();

        let orig_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&td).unwrap();
        let result = scan_locks().unwrap();
        std::env::set_current_dir(orig_dir).unwrap();

        let _: () = std::fs::remove_dir_all(&td).ok().unwrap_or(());
        let _: () = std::fs::remove_dir_all(&target_dir).ok().unwrap_or(());

        // Should find root and real locks, but not the symlinked lock
        assert_eq!(result.len(), 2);
    }
}
