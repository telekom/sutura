//! A narrow structural check for the CLI crate's composition-root claim.
//!
//! `sutura-cli` selects adapters and reports outcomes. A new error definition is a reviewable
//! signal that it may have taken on conversion or domain work. The one production error here is
//! the closed enum that erases the selected adapter's error; its path and name are pinned below.
//! Test-only code is excluded with the same region reader the causality gate uses.
//!
//! This is a proxy, not a proof that the crate contains no business logic. Code that adds logic
//! without defining an error passes; a derived error written by a macro, an aliased `Error` trait,
//! or `cfg_attr` is outside this scan.
//! The manifest description is pinned so changing the claim requires changing this check too.

use std::path::Path;

use crate::Verdict;
use crate::causality::regions;
use crate::repo;
use crate::serde_parse::scan::{code_lines, matching_angle};

const MANIFEST: &str = "crates/sutura-cli/Cargo.toml";
const SOURCE: &str = "crates/sutura-cli/src";
const DESCRIPTION: &str = "\"The sutura binary. Composes adapters; contains no business logic.\"";
const REQUIRED: &[&str] = &["crates/sutura-cli/src/import.rs", "crates/sutura-cli/src/serve/kind.rs"];
const ALLOWED: &[(&str, &str)] = &[("crates/sutura-cli/src/serve/kind.rs", "AnyWarehouseError")];

type ErrorDefinition = (String, String, usize);

pub(super) fn check() -> Verdict {
    let Some(root) = repo::root() else {
        eprintln!("xtask check-boundaries: FAILED - could not locate the composition root");
        return Verdict::Fail;
    };
    check_at(&root)
}

fn check_at(root: &Path) -> Verdict {
    let manifest = match std::fs::read_to_string(root.join(MANIFEST)) {
        Ok(text) => text,
        Err(why) => {
            eprintln!("xtask check-boundaries: FAILED - could not read {MANIFEST}: {why}");
            return Verdict::Fail;
        }
    };
    let descriptions: Vec<&str> = manifest
        .lines()
        .filter_map(|line| line.trim().strip_prefix("description = "))
        .collect();
    if descriptions.as_slice() != [DESCRIPTION] {
        eprintln!("xtask check-boundaries: FAILED - {MANIFEST} changed its composition-root description");
        return Verdict::Fail;
    }

    let census = repo::collect_files(root, &root.join(SOURCE), &["rs"]);
    let mut definitions: Vec<ErrorDefinition> = Vec::new();
    let mut read = 0_usize;
    let inspected = census.inspect(
        REQUIRED,
        |_| true,
        |rel, bytes| {
            read = read.saturating_add(1);
            let source = String::from_utf8_lossy(bytes);
            let code = code_lines(&source);
            let test_scope = regions::scope(rel, &|path| std::fs::read_to_string(root.join(path)).ok());
            for (line, name) in error_definitions(&code, &test_scope) {
                definitions.push((String::from(rel), name, line));
            }
        },
    );
    let inspected = match inspected {
        Ok(value) => value,
        Err(why) => {
            eprintln!("xtask check-boundaries: FAILED - composition-root scan: {}", why.describe());
            return Verdict::Fail;
        }
    };

    let mut problems = Vec::new();
    for (path, name, line) in &definitions {
        if !ALLOWED.contains(&(path.as_str(), name.as_str())) {
            problems.push(format!("{path}:{line}: `{name}` defines a new error in the composition root"));
        }
    }
    for &(path, name) in ALLOWED {
        if !definitions
            .iter()
            .any(|(found_path, found_name, _)| found_path == path && found_name == name)
        {
            problems.push(format!("{path}: the allowed adapter-error wrapper `{name}` is absent"));
        }
    }
    if problems.is_empty() {
        println!(
            "xtask check-boundaries: ok - composition root has only its declared adapter-error wrapper ({read} source file(s); {})",
            inspected.verdict()
        );
        Verdict::Pass
    } else {
        eprintln!("xtask check-boundaries: FAILED - composition-root error definitions:");
        for problem in problems {
            eprintln!("  {problem}");
        }
        Verdict::Fail
    }
}

fn error_definitions(code: &[String], tests: &regions::TestScope) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut deriving = String::new();
    let mut waiting_for_item = false;
    for (index, line) in code.iter().enumerate() {
        let number = index.saturating_add(1);
        if tests.covers(number) {
            deriving.clear();
            waiting_for_item = false;
            continue;
        }
        let trimmed = line.trim();
        if !deriving.is_empty() || trimmed.contains("#[derive(") {
            deriving.push_str(trimmed);
        }
        let item_line = if !deriving.is_empty() && deriving.contains(")]") {
            waiting_for_item = derives_error(&deriving);
            deriving.clear();
            trimmed.split_once(")]").map_or("", |(_, rest)| rest.trim())
        } else {
            trimmed
        };
        if waiting_for_item {
            if let Some(name) = item_name(item_line) {
                found.push((number, name));
                waiting_for_item = false;
            } else if !item_line.is_empty() && !item_line.starts_with("#[") {
                waiting_for_item = false;
            }
        }
        if let Some(name) = error_impl_name(trimmed) {
            found.push((number, String::from(name)));
        }
    }
    found
}

fn error_impl_name(line: &str) -> Option<&str> {
    let body = line
        .strip_prefix("impl")
        .or_else(|| line.split_once(" impl").map(|(_, body)| body))?
        .trim_start();
    let body = if body.starts_with('<') {
        body.get(matching_angle(body)?.saturating_add(1)..)?.trim_start()
    } else {
        body
    };
    for name in ["core::error::Error", "std::error::Error", "Error"] {
        if let Some(rest) = body.strip_prefix(name).and_then(|rest| rest.strip_prefix(" for ")) {
            return rest
                .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .next()
                .filter(|name| !name.is_empty());
        }
    }
    None
}

fn derives_error(attribute: &str) -> bool {
    let Some((_, rest)) = attribute.split_once("#[derive(") else {
        return false;
    };
    let Some((items, _)) = rest.split_once(')') else {
        return false;
    };
    items.split(',').any(|item| {
        let item = item.trim();
        item == "Error" || item.ends_with("::Error")
    })
}

fn item_name(line: &str) -> Option<String> {
    for keyword in ["enum ", "struct "] {
        if let Some((_, rest)) = line.split_once(keyword) {
            let name = rest.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_')).next()?;
            if !name.is_empty() {
                return Some(String::from(name));
            }
        }
    }
    None
}
