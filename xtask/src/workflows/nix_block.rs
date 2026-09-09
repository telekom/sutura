//! Reading a named block out of `flake.nix`, and saying so when the parse desynchronises.
//!
//! **Its own file because `xtask/src/workflows.rs` reached the 1000-line cap** - a cap `crates/`
//! and `xtask/` cannot be exempted from - the moment two changes registered a rule module in the
//! same registry in the same window. That is the merge-time contention `.agents/skills` already
//! records about `TASKS`, one level down, and the answer it prescribes is to split rather than to
//! shave lines off a comment.
//!
//! **The seam is real rather than convenient.** The parent walks CI's call graph and prints one
//! verdict; this answers a different question about a different file - *which attributes does this
//! Nix output block declare, and did the block close*. Three callers outside the parent ask it:
//! `crate::compose::file`, `crate::warm_start`, and the two badge rules. The lexer it reads
//! through, `super::code_lines`, stays with the parent: `crate::warm_start` asks that one directly
//! through `nix_code_lines`, so moving it would move a second caller's seam too.
//!
//! Every shape that once fooled this reader is recorded on the item it fooled - a brace on a
//! continuation line, a brace inside a comment, an indented string, a `let` binding, an
//! interpolation holding an attrset - and each has a test below. **A block that never closes is
//! `None`**: a gate whose parse has desynchronised says the parse is broken rather than answering
//! the question with a guess.

use std::collections::BTreeSet;

use super::code_lines;

/// Every attribute at the top level of an output block.
///
/// Depth-tracked rather than stopping at the first `};`. The first version broke out there and
/// so missed everything after the first NESTED close - which meant `checks.hygiene`, declared
/// well below `clippy`, read as undeclared while CI built it happily every run. A parser that
/// silently sees half a file is worse than no parser.
///
/// THE BLOCK'S OWN OPENING BRACE IS COUNTED rather than assumed to be on the header line, and
/// that is a second version of the same bug. `depth` used to be set to 1 the moment the header
/// matched, which is right only while the `{` is on that line: written as
///
/// ```text
/// packages = crossPackages // ociImages
///   // nativeImages // {
/// ```
///
/// the brace on the continuation line read as a NESTED attrset, so depth became 2 and every name
/// in the block was invisible - `packages.xtask` among them, which `ci.yml` runs three times.
/// Measured, on the change that split that line. Counting the header's braces like any other
/// line's makes both shapes the same case, and `opened` is what keeps the `depth <= 0` break from
/// firing before the block has started.
/// `pub(crate)` rather than private: `crate::compose::file` asks the same question of the same
/// block - which checks does `flake.nix` declare - and a second parser for it would be a second
/// thing to keep in step with the shapes this doc comment records.
///
/// `None` where the header matched and the block never closed. That case USED TO BE SILENT, and
/// silence is what made the comment-brace defect expensive: the scan ran off the end of the block
/// into the rest of `outputs`, so it reported `formatter` as a check, lost six real ones, and the
/// failure surfaced as eighteen workflow references that "do not exist". A gate whose parse has
/// desynchronised must say the parse is broken - never answer the question with a guess.
pub(crate) fn declared_block(text: &str, header: &str) -> Option<BTreeSet<String>> {
    scan_block(text, header).map(|(names, _)| names)
}

/// The RAW source of one output block, header line to closing line.
///
/// Raw and not the code projection, because the question its caller asks - does this block name
/// `sutura-<service>-tier` or a literal nextest selector - is about a value inside a string
/// literal, which the projection blanks out. Check-scope reads this same span at run time.
pub(crate) fn block_source(text: &str, header: &str) -> Option<String> {
    scan_block(text, header).map(|(_, source)| source)
}

/// One parsed output block: the attributes it declares, and its raw source.
///
/// An alias and not a struct, and that is the compiler choosing between two lints rather than a
/// style preference. `clippy::type_complexity` refuses the tuple written out; a struct puts the
/// source in a named field, whose only reader is `crate::compose::file` - a `#[cfg(test)] mod` -
/// so `dead_code` refuses that in the binary. The alias satisfies both without an `allow`.
type Block = (BTreeSet<String>, String);

/// One scan, shared by both faces above, because a second one would be a second thing to keep in
/// step with the shapes recorded here.
fn scan_block(text: &str, header: &str) -> Option<Block> {
    let mut names = BTreeSet::new();
    let mut depth = 0_i32;
    let mut inside = false;
    let mut opened = false;
    let mut block_closed = false;
    let mut source = String::new();
    let raw: Vec<&str> = text.lines().collect();
    for (number, line) in code_lines(text).into_iter().enumerate() {
        let trimmed = line.code.trim();
        let header_line = !inside && trimmed.starts_with(header);
        if header_line {
            inside = true;
        } else if !inside {
            continue;
        }
        if let Some(original) = raw.get(number) {
            source.push_str(original);
            source.push('\n');
        }

        // Only the outermost level of the block declares an output; everything deeper belongs
        // to one. Counted after the name check so the closing line of a nested attrset does
        // not look like a declaration, and never on the header line, which declares the block
        // rather than a member of it.
        let opens = i32::try_from(trimmed.matches('{').count()).unwrap_or(0);
        let closes = i32::try_from(trimmed.matches('}').count()).unwrap_or(0);

        if !header_line
            && opened
            && depth == 1
            && line.lets == 0
            && let Some((key, _)) = trimmed.split_once('=')
        {
            let key = key.trim();
            let plain = !key.is_empty()
                && !key.contains(' ')
                && !key.contains('.')
                && key.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if plain {
                names.insert(String::from(key));
            }
        }

        depth = depth.saturating_add(opens).saturating_sub(closes);
        if depth > 0 {
            opened = true;
        }
        if opened && depth <= 0 {
            block_closed = true;
            break;
        }
    }
    block_closed.then_some((names, source))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_block_whose_opening_brace_is_on_a_continuation_line_still_declares_its_members() {
        // The `packages = ` line in flake.nix grew past one line when a second shipped binary
        // was added, and the brace moved with it. Depth was pinned to 1 at the header, so the
        // brace on the second line read as a NESTED attrset and every member of the block became
        // invisible - including `xtask`, which `ci.yml` runs three times. This is that shape.
        let flake = concat!(
            "        packages = crossPackages // ociImages\n",
            "          // nativeImages // {\n",
            "          default = sutura;\n",
            "          xtask = craneLib.buildPackage (ciArgs // {\n",
            "            pname = \"xtask\";\n",
            "          });\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "packages = ").expect("the block closes");
        assert!(names.contains("xtask"), "xtask must be declared, got {names:?}");
        assert!(names.contains("default"), "default must be declared, got {names:?}");
        assert!(!names.contains("pname"), "a nested attribute is not a declaration");
    }

    #[test]
    fn a_block_whose_opening_brace_is_on_the_header_line_is_unchanged() {
        // The shape every other block in flake.nix has, asserted beside the one above so a fix
        // for one cannot quietly become a regression in the other.
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy (ciArgs // {\n",
            "            cargoArtifacts = ciArtifacts;\n",
            "          });\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert!(names.contains("clippy"), "got {names:?}");
        assert!(names.contains("hygiene"), "declared below a nested close, got {names:?}");
        assert!(!names.contains("cargoArtifacts"), "a nested attribute is not a declaration");
    }

    #[test]
    fn a_brace_inside_a_comment_does_not_shift_the_block() {
        // THE DEFECT THAT SHIPPED. A sentence in `flake.nix` quoting the block's own header in
        // prose - `checks = {` inside a `#` comment - was skipped when reading a name and counted
        // when counting depth, so everything below it sat one level too deep. Six checks that CI
        // builds every run became undeclared, and the gate reported eighteen workflow references
        // as pointing at outputs that do not exist.
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy { };\n",
            "          # `checks = {` is the block two xtask gates read, so it stays here.\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
            "        };\n",
            "        formatter = pkgs.nixfmt;\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert!(names.contains("hygiene"), "a comment's brace must not hide it, got {names:?}");
        assert!(!names.contains("formatter"), "the scan ran past the block, got {names:?}");
        assert_eq!(names.len(), 2, "got {names:?}");
    }

    #[test]
    fn shell_inside_an_indented_string_declares_nothing() {
        // `checks.keycloak-tier`'s body is an inline shell script. At raw-text depth its
        // assignments sat at the block's own level, so `port` and `realm` were reported as
        // declared checks - a gate INVENTING outputs, which is worse than losing them because a
        // caller cannot tell the difference. The `${...}` is code and its braces still balance.
        let flake = concat!(
            "        checks = {\n",
            "          keycloak-tier = pkgs.runCommand \"k\" { } ''\n",
            "            port=\"$(jq -r '.port' \"$f\")\"\n",
            "            realm=.sutura-dev/keycloak-realm.json\n",
            "            case \"$x\" in *a*) echo ${tier.realm} ;; esac\n",
            // A brace shell leaves unbalanced, which is the half a `matches('{').count()` cannot
            // survive at all: one of these shifts every line below it.
            "            sed -n 's|.*}||p' \"$log\"\n",
            "          '';\n",
            "          fmt = craneLib.cargoFmt { };\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "shell text is not a declaration, got {names:?}");
        assert!(names.contains("keycloak-tier"), "got {names:?}");
        assert!(
            names.contains("fmt"),
            "an unbalanced shell brace must not hide it, got {names:?}"
        );
    }

    #[test]
    fn a_let_binding_inside_a_check_is_not_a_check() {
        // `let` opens no brace, so a binding in a check's own value is at the same brace depth as
        // the check. Seven of them read as declared outputs on `main` for as long as the release
        // checks were written inline in `flake.nix`.
        let flake = concat!(
            "        checks = {\n",
            "          one-binary =\n",
            "            let\n",
            "              cells = map f binaries;\n",
            "              checkOne = p: \"x\";\n",
            "            in\n",
            "            pkgs.runCommand \"one\" { } \"\";\n",
            "          reuse = pkgs.runCommand \"r\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "a let binding is not an output, got {names:?}");
        assert!(names.contains("one-binary"), "got {names:?}");
        assert!(names.contains("reuse"), "got {names:?}");
    }

    #[test]
    fn a_block_that_never_closes_is_an_error_and_not_an_answer() {
        // The direction this gate has to fail in. Answering with the names it happened to collect
        // is how a desynchronised parse became eighteen confusing reference failures instead of
        // one clear "the scan is broken".
        let flake = concat!(
            "        checks = {\n",
            "          clippy = craneLib.cargoClippy { };\n",
            "          hygiene = pkgs.runCommand \"h\" { } \"\";\n",
        );
        assert!(
            super::declared_block(flake, "checks = {").is_none(),
            "an unclosed block must not answer the question"
        );
    }

    #[test]
    fn an_interpolation_holding_an_attrset_keeps_the_scan_aligned() {
        // `${pkgs.closureInfo { rootPaths = [ drv ]; }}` - a `${` whose code contains its own
        // braces. Popping the interpolation on the FIRST `}` swallows the second, and the block
        // then never closes.
        let flake = concat!(
            "        checks = {\n",
            "          one-binary = pkgs.runCommand \"o\" { } ''\n",
            "            grep -q x ${pkgs.closureInfo { rootPaths = [ drv ]; }}/store-paths\n",
            "          '';\n",
            "          reuse = pkgs.runCommand \"r\" { } \"\";\n",
            "        };\n",
        );
        let names = super::declared_block(flake, "checks = {").expect("the block closes");
        assert_eq!(names.len(), 2, "got {names:?}");
        assert!(names.contains("reuse"), "the scan lost alignment, got {names:?}");
    }

    #[test]
    fn the_real_flake_declares_the_checks_ci_builds() {
        // The unit fixtures above are shapes; this is the file. Anchored on names `ci.yml` and
        // `justfile` both build, so a parse that regresses on the real tree fails here rather
        // than in a nix step minutes into a run.
        let Some(root) = crate::repo::root() else { return };
        let Ok(flake) = std::fs::read_to_string(root.join("flake.nix")) else {
            return;
        };
        let names = super::declared_block(&flake, "checks = {").expect("flake.nix's `checks = {` block must close");
        for required in ["clippy", "nextest", "hygiene", "fmt", "doctest", "crap", "api-docs"] {
            assert!(names.contains(required), "`checks.{required}` is not declared, got {names:?}");
        }
        assert!(
            !names.contains("formatter"),
            "`formatter` is a sibling of `checks`, so the scan ran past the block: {names:?}"
        );
    }
}
