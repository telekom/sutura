//! Reading a Nix file for the two facts the pairing is made of: a binding, and where it sits.
//!
//! Separated from the claim so each half can be exercised on its own. The primitives here have no
//! tree to read and no opinion about the sweep - they answer *is this identifier BOUND here*,
//! *what encloses this offset*, and *what does this `inherit` name* - which is the half of a gate
//! that is otherwise only ever run green through the verdict above it.
//!
//! ONE VIEW PER QUESTION, and the split is deliberate. A nix BINDING is read off [`NixFile::code`],
//! the lexed half, because `flake.nix`'s own comments discuss `cargoArtifacts` and `preBuild` in
//! prose and `nix/mimalloc.nix` writes `runHook preBuild` inside a builder script: a raw scan
//! reports all three. A SHELL line inside an indented string is read off [`NixFile::raw`] through
//! [`crate::warm_start::live_lines`], because the lexer blanks exactly that text - so the warmer's
//! `export` is invisible in the code half, and *a line that STARTS with `#` runs nothing* is the
//! right rule for a string that is shell either way.
//!
//! THE LEXER IS `workflows`', NOT A SECOND ONE. Its own doc comment lists the three shapes that
//! fooled the brace count it replaced, and a comparison policy copied into a second consumer is a
//! control that will diverge or be wrong twice.

use std::path::Path;

use crate::workflows;

/// One `.nix` file, in both of the views this gate needs.
pub(super) struct NixFile {
    /// Repo-relative, with `/` separators - what a reader can open.
    pub(super) rel: String,
    /// The file as written. The shell inside an indented string lives here and nowhere else.
    pub(super) raw: String,
    /// The nix CODE half, comments and string interiors blanked by [`workflows::nix_code_lines`],
    /// interpolations kept. Lines are joined back up so an offset in it has a line number.
    pub(super) code: String,
}

/// Both views, taken once: a second read of the same file is a second answer to keep in step.
impl NixFile {
    pub(super) fn read(root: &Path, rel: &str) -> Result<Self, String> {
        let raw = std::fs::read_to_string(root.join(rel)).map_err(|error| format!("could not read {rel}: {error}"))?;
        let code = workflows::nix_code_lines(&raw).join("\n");
        Ok(Self {
            rel: String::from(rel),
            raw,
            code,
        })
    }
}

/// Is this the whole identifier, rather than the tail of a longer one?
pub(super) fn whole_word(code: &str, start: usize, end: usize) -> bool {
    let word = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '\'');
    let before = code.get(..start).and_then(|head| head.chars().next_back());
    let after = code.get(end..).and_then(|tail| tail.chars().next());
    !before.is_some_and(word) && !after.is_some_and(word)
}

/// Every byte offset in `code` at which `name` is BOUND - the identifier followed by an `=`.
///
/// A binding and a use are different facts and this gate is about bindings: `${cargoArtifacts}`
/// inside the warmer's shell is a use, `{ pkgs, cargoArtifacts, .. }:` is a parameter, and
/// `inheritCargoArtifacts` is crane's own function. None of the three hands artifacts to anything.
pub(super) fn bound_at(code: &str, name: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = code.get(from..).and_then(|rest| rest.find(name)) {
        let start = from.saturating_add(offset);
        let end = start.saturating_add(name.len());
        from = end;
        if !whole_word(code, start, end) {
            continue;
        }
        let tail = code.get(end..).unwrap_or_default().trim_start();
        if tail.starts_with('=') && !tail.starts_with("==") {
            found.push(start);
        }
    }
    found
}

/// The 1-based line an offset in the code half sits on.
pub(super) fn line_at(code: &str, offset: usize) -> usize {
    code.get(..offset).unwrap_or_default().matches('\n').count().saturating_add(1)
}

/// The first balanced `{ .. }` at or after `from`, as a byte range.
///
/// An unclosed brace is `None` and therefore an ERROR at the caller, never an answer: this file's
/// neighbours record three gates that counted braces and reported a parse failure as a verdict.
pub(super) fn attrset_at(code: &str, from: usize) -> Option<(usize, usize)> {
    let open = code.get(from..)?.find('{')?.saturating_add(from);
    let mut depth = 0_usize;
    for (offset, character) in code.get(open..)?.char_indices() {
        match character {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((open, open.saturating_add(offset).saturating_add(1)));
                }
            }
            _ => {}
        }
    }
    None
}

/// The `{` of the innermost attrset enclosing `offset`.
pub(super) fn enclosing_attrset(code: &str, offset: usize) -> Option<usize> {
    let mut pending = 0_usize;
    for (at, character) in code.get(..offset)?.char_indices().rev() {
        match character {
            '}' => pending = pending.saturating_add(1),
            '{' if pending == 0 => return Some(at),
            '{' => pending = pending.saturating_sub(1),
            _ => {}
        }
    }
    None
}

/// The path of the module whose argument set opens at `brace`, if that is what this is.
///
/// `inherit (import ./nix/cargo-env.nix {` is the shape in the tree; the leading `(` is trimmed so
/// the two tokens compared are `import` and a relative path.
pub(super) fn imported_at(code: &str, brace: usize) -> Option<&str> {
    let prefix = code.get(..brace)?.trim_end();
    let mut words = prefix.split_whitespace().rev();
    let path = words.next()?;
    if !path.starts_with("./") {
        return None;
    }
    if words.next()?.trim_start_matches('(') != "import" {
        return None;
    }
    Some(path)
}

/// Every offset at which an `inherit` statement names `name`.
///
/// `inherit cargoArtifacts;` hands the artifacts on with **no `=` anywhere**, so [`bound_at`]
/// passes over it - the shape a `while read` loop was to the shipped-binaries gate, written in
/// nix. It is discovered as a taking and never attributed: an `inherit` names a binding in some
/// enclosing scope, this gate follows no scopes, and *cannot say* is the honest answer rather than
/// a pass. Nothing in this tree writes it, which is what a refusal over a shape nobody writes
/// should do.
///
/// THE `inherit (expr) names;` FORM IS WHY THIS IS NOT A `contains`. `flake.nix` writes
/// `inherit (import ./nix/cargo-env.nix { cargoArtifacts = ciArtifacts; .. }) cargoLinkEnv ..;` -
/// a real taking, already attributed to the module by its enclosing attrset - and reading the text
/// up to the `;` would report it a second time as an unattributable one, reddening the true tree.
/// So a leading parenthesised expression is skipped over and only the NAMES are read.
pub(super) fn inherited_at(code: &str, name: &str) -> Vec<usize> {
    const INHERIT: &str = "inherit";
    let mut found = Vec::new();
    let mut from = 0_usize;
    while let Some(offset) = code.get(from..).and_then(|rest| rest.find(INHERIT)) {
        let start = from.saturating_add(offset);
        let mut at = start.saturating_add(INHERIT.len());
        from = at;
        if !whole_word(code, start, at) {
            continue;
        }
        // Skip a `(expr)` source, balanced, so the names are what is read.
        let tail = code.get(at..).unwrap_or_default();
        let lead = tail.len().saturating_sub(tail.trim_start().len());
        if tail.trim_start().starts_with('(') {
            let mut depth = 0_usize;
            let mut end = None;
            for (index, character) in tail.get(lead..).unwrap_or_default().char_indices() {
                match character {
                    '(' => depth = depth.saturating_add(1),
                    ')' => {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            end = Some(index.saturating_add(1));
                            break;
                        }
                    }
                    _ => {}
                }
            }
            // An unbalanced source is not an answer: leave it to the binding scan rather than
            // guessing where the names start.
            let Some(end) = end else { continue };
            at = at.saturating_add(lead).saturating_add(end);
        }
        let names = code.get(at..).unwrap_or_default();
        let names = names.get(..names.find(';').unwrap_or(names.len())).unwrap_or_default();
        let mut cursor = 0_usize;
        for word in names.split_whitespace() {
            let Some(within) = names.get(cursor..).and_then(|rest| rest.find(word)) else {
                break;
            };
            let word_at = cursor.saturating_add(within);
            cursor = word_at.saturating_add(word.len());
            if word == name {
                found.push(at.saturating_add(word_at));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    /// The shapes that NAME the identifier and bind nothing, every one of them in the real tree.
    const DECOYS: &str = concat!(
        "  # A comment naming cargoArtifacts = something, which is prose.\n",
        "  { pkgs, duckdb, cargoArtifacts, cargoVendorDir }:\n",
        "  warm = ''\n",
        "    echo \"${cargoArtifacts}/target.tar.zst\"\n",
        "  '';\n",
        "  inheritCargoArtifacts = true;\n",
    );

    fn lexed(text: &str) -> String {
        crate::workflows::nix_code_lines(text).join("\n")
    }

    #[test]
    fn a_use_a_parameter_and_a_longer_identifier_are_not_bindings() {
        // Each decoy is one of this tree's own shapes: a comment discussing the name, the module's
        // formal parameter, an interpolation in shell text, and crane's function whose name ENDS
        // with it. A `contains` counts four; a binding scan counts none.
        let code = lexed(DECOYS);
        assert_eq!(super::bound_at(&code, "cargoArtifacts"), Vec::<usize>::new());
        // And the binding IS found, wherever the `=` sits relative to the newline.
        let bound = lexed("  cargoArtifacts = artifacts;\n  other =\n    cargoArtifacts;\n");
        assert_eq!(super::bound_at(&bound, "cargoArtifacts").len(), 1);
        let wrapped = lexed("  cargoArtifacts\n    = artifacts;\n");
        assert_eq!(super::bound_at(&wrapped, "cargoArtifacts").len(), 1);
    }

    #[test]
    fn an_inherit_names_the_identifier_but_an_inherit_from_an_expression_does_not() {
        // The two `inherit` forms, and the second is why this is not a `contains`: `flake.nix`
        // writes a real taking INSIDE the parentheses of an `inherit (import ..) names;`, already
        // attributed by its enclosing attrset, and reading to the `;` would report it twice.
        let plain = lexed("  hygiene = args // { inherit cargoArtifacts; };\n");
        assert_eq!(super::inherited_at(&plain, "cargoArtifacts").len(), 1);
        let from_expression = lexed(concat!(
            "  inherit (import ./nix/cargo-env.nix {\n",
            "    inherit pkgs duckdb;\n",
            "    cargoArtifacts = ciArtifacts;\n",
            "  }) cargoLinkEnv cargoWarmStart;\n",
        ));
        assert_eq!(super::inherited_at(&from_expression, "cargoArtifacts"), Vec::<usize>::new());
        // The real tree writes exactly that form, so a false positive here reddens `main`.
        let root = crate::repo::root().expect("the repo root");
        let flake = super::NixFile::read(&root, "flake.nix").expect("flake.nix");
        assert_eq!(super::inherited_at(&flake.code, "cargoArtifacts"), Vec::<usize>::new());
    }

    #[test]
    fn the_brace_walks_agree_about_one_attrset() {
        // Forwards and backwards over the same nesting: `attrset_at` from a binding finds the set
        // that binding OPENS, and `enclosing_attrset` from inside finds the one that contains it.
        let code = lexed("a = x: {\n  b = { c = 1; };\n  d = 2;\n};\n");
        let at = super::bound_at(&code, "a").first().copied().expect("a is bound");
        let (open, close) = super::attrset_at(&code, at).expect("a's attrset closes");
        let inner = super::bound_at(&code, "c").first().copied().expect("c is bound");
        assert!(inner > open && inner < close, "c is inside a's attrset");
        let around = super::enclosing_attrset(&code, inner).expect("c is enclosed");
        assert!(around > open, "the INNERMOST set encloses c, not a's");
        // An unclosed block is None - an error at the caller - rather than a range to reason with.
        assert_eq!(super::attrset_at(&lexed("a = {\n  b = 1;\n"), 0), None);
        assert_eq!(super::enclosing_attrset(&lexed("a = 1;\n"), 3), None);
    }

    #[test]
    fn the_line_a_binding_sits_on_is_the_line_the_file_has() {
        // A message naming the wrong line is unactionable, and the lexer's blanking is what makes
        // this hold: it replaces characters rather than dropping them, so the count survives.
        let code = lexed("# one\n# two\ncargoArtifacts = x;\n");
        let at = super::bound_at(&code, "cargoArtifacts").first().copied().expect("bound");
        assert_eq!(super::line_at(&code, at), 3);
    }

    #[test]
    fn only_an_import_of_a_relative_path_attributes_an_attrset() {
        let code = lexed("  inherit (import ./nix/cargo-env.nix {\n    cargoArtifacts = x;\n  }) y;\n");
        let at = super::bound_at(&code, "cargoArtifacts").first().copied().expect("bound");
        let brace = super::enclosing_attrset(&code, at).expect("enclosed");
        assert_eq!(super::imported_at(&code, brace), Some("./nix/cargo-env.nix"));
        // Not an import, and an import of something that is not a relative path: both unattributed
        // rather than guessed at, which is what makes the caller's refusal the safe direction.
        let plain = lexed("  args // {\n    cargoArtifacts = x;\n  };\n");
        let at = super::bound_at(&plain, "cargoArtifacts").first().copied().expect("bound");
        let brace = super::enclosing_attrset(&plain, at).expect("enclosed");
        assert_eq!(super::imported_at(&plain, brace), None);
        let flake_input = lexed("  (import nixpkgs {\n    cargoArtifacts = x;\n  });\n");
        let at = super::bound_at(&flake_input, "cargoArtifacts")
            .first()
            .copied()
            .expect("bound");
        let brace = super::enclosing_attrset(&flake_input, at).expect("enclosed");
        assert_eq!(super::imported_at(&flake_input, brace), None);
    }
}
