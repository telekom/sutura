//! Two text views over the same census read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{TreeVerdict, tree_problems};

/// Existing checks retain lossy decoding; host and page checks use the strict view their former
/// `read_to_string` calls provided. Both views come from one read inside `Census::inspect`.
#[derive(Default)]
pub(in crate::guidance) struct Texts {
    lossy: BTreeMap<String, String>,
    invalid: BTreeSet<String>,
}

impl Texts {
    pub(super) fn insert(&mut self, rel: &str, bytes: &[u8]) {
        let text = if let Ok(valid) = std::str::from_utf8(bytes) {
            String::from(valid)
        } else {
            self.invalid.insert(String::from(rel));
            String::from_utf8_lossy(bytes).into_owned()
        };
        self.lossy.insert(String::from(rel), text);
    }

    pub(in crate::guidance) fn get(&self, rel: &str) -> Option<&String> {
        self.lossy.get(rel)
    }

    pub(in crate::guidance) fn strict(&self, rel: &str) -> Option<String> {
        if self.invalid.contains(rel) {
            None
        } else {
            self.lossy.get(rel).cloned()
        }
    }
}

pub(super) fn tree_problems_from_texts(root: &Path, texts: &Texts, files: &[String], text_files: &[String]) -> TreeVerdict {
    let lossy = |rel: &str| texts.get(rel).cloned();
    let strict = |rel: &str| texts.strict(rel);
    tree_problems(root, &lossy, &strict, files, text_files)
}
