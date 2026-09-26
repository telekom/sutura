//! The structural half of the ungoverned-route allowlist.
//!
//! `crate::router::{Ungoverned, check_ungoverned}` hold the allowlist at the TYPE: `Ungoverned::mount`
//! fuses a subtree with the path it was mounted at, and no other method on that type hands back a
//! bare, re-fusable `Router` - see that type's own doc for what holds. This half is what makes "the
//! only way to mount anything outside the governed subtree" true of the SOURCE rather than of a
//! caller who never reaches for `Ungoverned` at all - the backstop that closes `#758` M6 (a subtree
//! merged with no `Ungoverned` in sight). It refuses any
//! `.nest`/`.nest_service`/`.route_service`/`.fallback_service`, and any `.route(`, in `sutura-http`
//! or `sutura-cli` except inside `Ungoverned::mount`, inside a `#[cfg(test)] mod tests` block, at a
//! site in [`ALLOWED_ROUTES`], or in a file whose path ends in `tests.rs` (by naming convention -
//! see the limit below), and except the governed subtree's own
//! `.nest(API_V1_PREFIX, …)`, which `crate::router::governed_routes`/`every_route_governed` holds by
//! a different mechanism.
//!
//! **Deliberately NOT a needle: `.merge(`.** `Ungoverned`'s own `merge_into` and `assemble`'s merges
//! of the documentation/metrics/versioned/public subtrees, plus `sutura-http/src/openapi.rs`'s merge
//! of an `utoipa::openapi::OpenApi` DOCUMENT (a different type entirely, same method name), are all
//! legitimate `.merge(` call sites this scan cannot tell from a future one that fuses in an
//! ungoverned router bare - a needle broad enough to catch the latter is broad enough to also refuse
//! the former, with no per-call-site type information available to a text scan. `#758`'s round-2
//! review measured that exact bare-`.merge` shape (`Ungoverned::router()` returning a bare `Router`
//! for a caller to merge outside `merge_into`) passing this gate; the remedy taken was removing that
//! accessor from `Ungoverned` and `AgentMount` entirely, so the shape no longer compiles - a type
//! fix, stronger than a gate could be made here without an allowlist of caller identities.
//!
//! **Limit, stated rather than hidden.** The needle is text over source: a route whose `.route(`
//! token is hidden - macro-expanded, as `.routes(utoipa_axum::routes!(..))` mounts `/health` and
//! `/metrics` today, or a helper not spelling the token - is invisible, and a new route mounted
//! that way is held by review alone. It is line-oriented with a `//`-comment
//! strip and no string interior is blanked, and it tracks "inside `Ungoverned::mount`" and "inside
//! `impl Ungoverned`" by brace depth, so a mount spread across lines whose needle is rewritten to
//! land on a line outside the tracked depth would escape it. The `.route(` needle fires when the
//! token is on the line; a first argument collected across a line break is read only up to the
//! first top-level comma (string interiors and nested `()`/`[]`/`{}` are skipped, so a comma inside
//! a `"/a,b"` path or a nested call's argument is not a separator), so a route whose path argument
//! spans a line break after that comma is matched by its first line only. A line shift in an
//! allowlisted file moves the record → red gate (safe: forces re-record). The `tests.rs` exemption is
//! by naming convention, not by a `#[cfg(test)]` check: this scan processes one file at a time and
//! cannot read a parent's `#[cfg(test)] mod tests;` declaration, so a production file named
//! `tests.rs` would be exempted too - held by repo convention and review, not by this gate. The
//! type half is the primary guard this scan backstops.

use crate::Verdict;

/// Runs the scan over both serving crates, and refuses a mount primitive anywhere but the mechanism.
pub(crate) fn check() -> Verdict {
    let Some(root) = crate::repo::root() else {
        eprintln!("xtask check-boundaries: could not find the workspace root");
        return Verdict::Fail;
    };
    let mut problems: Vec<String> = Vec::new();
    let mut files = 0usize;
    for base in ["crates/sutura-http/src", "crates/sutura-cli/src"] {
        let dir = root.join(base);
        if !dir.is_dir() {
            eprintln!("xtask check-boundaries: FAILED - `{base}` is not a directory; the ungoverned-mount scan reads nothing");
            return Verdict::Fail;
        }
        let mut stack = vec![dir];
        while let Some(path) = stack.pop() {
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(why) => {
                    problems.push(format!("{}: unreadable directory: {why}", path.display()));
                    continue;
                }
            };
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    stack.push(entry_path);
                } else if entry_path.extension().is_some_and(|ext| ext == "rs") {
                    let rel = entry_path
                        .strip_prefix(&root)
                        .map_or_else(|_| entry_path.display().to_string(), |p| p.display().to_string());
                    let text = match std::fs::read_to_string(&entry_path) {
                        Ok(text) => text,
                        Err(why) => {
                            problems.push(format!("{rel}: unreadable: {why}"));
                            continue;
                        }
                    };
                    files = files.saturating_add(1);
                    let mut scan = MountSiteScan::new(&rel);
                    for (index, line) in text.lines().enumerate() {
                        if let Some(reason) = scan.feed(line) {
                            problems.push(format!("{rel}:{}: {reason}", index.saturating_add(1)));
                        }
                    }
                }
            }
        }
    }
    if files == 0 {
        eprintln!("xtask check-boundaries: FAILED - ungoverned-mount-sites scanned no source file; the rule reads nothing");
        return Verdict::Fail;
    }
    if !problems.is_empty() {
        eprintln!(
            "xtask check-boundaries: FAILED - a route is mounted outside the governed subtree and outside `Ungoverned::mount`:"
        );
        for problem in &problems {
            eprintln!("  {problem}");
        }
        eprintln!();
        eprintln!("Every `.nest`/`.nest_service`/`.route_service`/`.fallback_service`, and any `.route`, in");
        eprintln!("`sutura-http` or `sutura-cli` must be the governed subtree's own");
        eprintln!("`.nest(API_V1_PREFIX, ...)`, inside `crate::router::Ungoverned::mount`, inside a");
        eprintln!("`#[cfg(test)] mod tests` block, in a file ending in `tests.rs`, or recorded in `ALLOWED_ROUTES`.");
        return Verdict::Fail;
    }
    println!("xtask check-boundaries: ok - every ungoverned mount lives inside `Ungoverned::mount` ({files} source file(s))");
    // Named on GREEN, the convention `check-boundaries` and `check-bounded-wait` state for the same
    // reason: an allowlist a green run never mentions is one nobody re-reads.
    for entry in ALLOWED_ROUTES {
        println!("  allowed  `{}` in {} - {}", entry.key, entry.file, entry.reason);
    }
    Verdict::Pass
}

/// A production `.route(` site that is neither inside `Ungoverned::mount` nor inside a test module,
/// recorded so the scan can tell it from an unrecorded one.
struct AllowedRoute {
    /// The file suffix the path ends with, so a prefix-join or a different workspace root does not
    /// move the record.
    file: &'static str,
    /// The trimmed text of the route's first argument, as it appears in the source. Line-independent:
    /// a line shift in the file does not change the argument text, so the record stays valid until
    /// the argument itself is edited (which is the change that should force re-record).
    key: &'static str,
    /// Why this route is outside the mechanism and still legitimate.
    reason: &'static str,
}

/// The two production `.route(` call sites this scan allows outside `Ungoverned::mount`:
///
/// * `router.rs` - `/openapi.json` via the `OPENAPI_JSON_PATH` constant, the generated interface
///   description served by `documentation()`.
/// * `routes/protected_resource.rs` - `&self.path`, the RFC 9728 well-known resource metadata route
///   served by `ProtectedResource::router()`.
///
/// Both are unauthenticated public routes mounted at the top level of `assemble`, outside the
/// version prefix and outside `Ungoverned::mount`, and both are deliberate. A new one is a visible
/// diff to this table with the argument in it.
const ALLOWED_ROUTES: &[AllowedRoute] = &[
    AllowedRoute {
        file: "sutura-http/src/router.rs",
        key: "OPENAPI_JSON_PATH",
        reason: "the generated interface description, served by `documentation()`",
    },
    AllowedRoute {
        file: "sutura-http/src/routes/protected_resource.rs",
        key: "&self.path",
        reason: "RFC 9728 well-known resource metadata, served by `ProtectedResource::router()`",
    },
];

/// The stateful line scanner behind [`check`].
///
/// Tracks brace depth and whether the walk is inside `Ungoverned::mount`'s body or a
/// `#[cfg(test)] mod tests` block, so a mount primitive is judged against the mechanism rather than
/// against a file name. It also knows the file's relative path, so a file whose path ends in
/// `tests.rs` is treated as test code (by naming convention - see the module's limit note) and a
/// `.route(` hit can be checked against [`ALLOWED_ROUTES`].
struct MountSiteScan {
    rel: String,
    /// `true` when `rel` ends in `/tests.rs` or is exactly `tests.rs`. By naming convention, not a
    /// `#[cfg(test)]` check - a production file named `tests.rs` would be exempted too (see the
    /// module's limit note).
    is_tests_file: bool,
    depth: usize,
    /// The depth `impl Ungoverned`'s body opens at, cleared once the walk leaves it - so a later
    /// `impl` (or a `fn mount_other` in this one) is never mistaken for the exempted site.
    impl_depth: Option<usize>,
    /// Waiting for a `{` that opens a tracked body. `Mount` waits for `fn mount`'s body (its
    /// signature may span a `where` clause); `CfgTest` waits for the `mod tests` after `#[cfg(test)]`;
    /// `TestMod` waits for the `{` that opens a `#[cfg(test)] mod tests` body the formatter split.
    pending: Pending,
    in_mount: Option<usize>,
    /// The depth a `#[cfg(test)] mod tests` body opens at, cleared once the walk leaves it.
    in_test_mod: Option<usize>,
    /// A `.route(` whose first argument spans to a later line: the argument text collected so far,
    /// waiting for the comma that ends it.
    pending_route_arg: Option<String>,
}

/// What the scanner is waiting for a `{` (or a `mod tests`) to open, if anything.
#[derive(Default, PartialEq, Eq)]
enum Pending {
    /// Not waiting for anything.
    #[default]
    None,
    /// `fn mount` signature seen, waiting for the body's `{` (may span a `where` clause).
    Mount,
    /// `#[cfg(test)]` seen, waiting for the `mod tests` it applies to.
    CfgTest,
    /// `#[cfg(test)] mod tests` seen, waiting for the body's `{` (formatter may split it).
    TestMod,
}

impl MountSiteScan {
    fn new(rel: &str) -> Self {
        Self {
            rel: rel.to_owned(),
            is_tests_file: rel.ends_with("/tests.rs") || rel == "tests.rs",
            depth: 0,
            impl_depth: None,
            pending: Pending::None,
            in_mount: None,
            in_test_mod: None,
            pending_route_arg: None,
        }
    }

    const fn in_mount(&self) -> bool {
        matches!(self.in_mount, Some(m) if self.depth >= m)
    }

    const fn in_impl(&self) -> bool {
        matches!(self.impl_depth, Some(d) if self.depth >= d)
    }

    const fn in_test_mod(&self) -> bool {
        matches!(self.in_test_mod, Some(d) if self.depth >= d)
    }

    /// `true` when this line's `.route(` call is at a site the allowlist records.
    fn route_line_is_allowed(&self, code: &str) -> bool {
        let Some(arg) = route_first_arg(code) else {
            return false;
        };
        self.arg_is_allowed(arg)
    }

    /// `true` when the already-extracted first argument matches an allowlist entry for this file.
    fn arg_is_allowed(&self, arg: &str) -> bool {
        ALLOWED_ROUTES
            .iter()
            .any(|entry| self.rel.ends_with(entry.file) && arg.trim() == entry.key)
    }

    /// Judge one line (without its trailing newline), returning a reason if it mounts an ungoverned
    /// route outside the mechanism. State for the next line is derived from this line.
    ///
    /// A `.route(` whose first argument spans to a later line is not judged on the opening line:
    /// the argument text is collected until the comma that ends it, and the violation (if any) is
    /// returned on the line where the argument completes. This is so an allowlisted multi-line
    /// `.route(` (the shape `router.rs` and `protected_resource.rs` carry) is not reported as a
    /// violation before its argument can be checked against the allowlist.
    fn feed(&mut self, line: &str) -> Option<&'static str> {
        let code = strip_line_comment(line);

        // A pending `.route(` argument carries over: collect until the first comma, then judge the
        // collected argument against the allowlist.
        if let Some(mut arg) = self.pending_route_arg.take() {
            if let Some((head, _tail)) = split_at_top_level_comma(code) {
                arg.push_str(head);
                // Now the argument is complete. The route is allowed inside `Ungoverned::mount`,
                // inside a test module, in a `tests.rs` file, or at an allowlisted site.
                let allowed = self.in_mount() || self.in_test_mod() || self.is_tests_file || self.arg_is_allowed(&arg);
                if !allowed {
                    self.update_state(line, code);
                    return Some("mounted outside the governed subtree and outside `Ungoverned::mount`");
                }
                // Allowed: no violation. Fall through to state update.
            } else {
                arg.push_str(code.trim_start());
                self.pending_route_arg = Some(arg);
                self.update_state(line, code);
                return None;
            }
        }

        let has_route = is_route_mount(code);
        let needle = code.contains(".nest(")
            || code.contains(".nest_service(")
            || code.contains(".route_service(")
            || code.contains(".fallback_service(")
            || has_route;
        let governed = code.contains(".nest(API_V1_PREFIX");

        // A `.route(` whose first argument is not on this line: defer the violation to the line where
        // the argument completes (so the allowlist can clear it). Other needles are judged now.
        if has_route && route_first_arg(code).is_none() {
            self.pending_route_arg = Some(String::new());
            self.update_state(line, code);
            return None;
        }

        // A `.route(` hit is allowed inside `Ungoverned::mount`, inside a test module, in a `tests.rs`
        // file, or at an allowlisted site. Other mount primitives (`.nest` etc.) are allowed only
        // inside `Ungoverned::mount` or as the governed subtree.
        let allowed = if has_route {
            self.in_mount() || self.in_test_mod() || self.is_tests_file || self.route_line_is_allowed(code)
        } else {
            self.in_mount()
        };
        let violation = needle && !governed && !allowed;

        self.update_state(line, code);

        violation.then_some("mounted outside the governed subtree and outside `Ungoverned::mount`")
    }

    /// Update the depth-tracking state for the next line from this line's code.
    fn update_state(&mut self, line: &str, code: &str) {
        // `in_impl` is read BEFORE this line's own `impl Ungoverned` (if any) opens it, so the line
        // naming the impl is not itself "in" it.
        let in_impl = self.in_impl();
        if code.contains("impl Ungoverned") {
            self.impl_depth = Some(self.depth + 1);
        }
        if in_impl {
            if code.contains("fn mount(") || code.contains("fn mount<") {
                // The body opens at one deeper than this line, whether the `{` is on this line
                // (short signature) or on a later line (a `where` clause spans it).
                if line.contains('{') {
                    self.in_mount = Some(self.depth + 1);
                    self.pending = Pending::None;
                } else {
                    self.pending = Pending::Mount;
                }
            } else if self.pending == Pending::Mount && line.contains('{') {
                self.in_mount = Some(self.depth + 1);
                self.pending = Pending::None;
            }
        }

        // `#[cfg(test)] mod tests` tracking, depth-tracked like `in_mount`. The attribute may be on
        // the same line as `mod tests {` or on the line above it; and the `{` may open on this line
        // or on a later one (rustfmt splits `mod tests\n{`).
        let trimmed = code.trim_start();
        let starts_cfg_test = trimmed.starts_with("#[cfg(test)]");
        if starts_cfg_test {
            self.pending = Pending::CfgTest;
        }
        let after_cfg = self.pending == Pending::CfgTest;
        let mod_tests_inline = code.contains("mod tests") && code.contains('{');
        let mod_tests_declared = code.contains("mod tests") && code.contains(';');
        if after_cfg && (mod_tests_inline || (code.contains("mod tests") && !mod_tests_declared)) {
            // `#[cfg(test)] mod tests {` (same line) or `#[cfg(test)]\nmod tests\n{` (next line).
            if line.contains('{') {
                self.in_test_mod = Some(self.depth + 1);
                self.pending = Pending::None;
            } else {
                self.pending = Pending::TestMod;
            }
        } else if self.pending == Pending::TestMod && line.contains('{') {
            self.in_test_mod = Some(self.depth + 1);
            self.pending = Pending::None;
        } else if after_cfg && mod_tests_declared {
            // `#[cfg(test)] mod tests;` - an out-of-line declaration. The child file is test code,
            // handled by `is_tests_file` when this scan processes it; nothing to track here.
            self.pending = Pending::None;
        }

        self.depth = self.depth.saturating_add(line.matches('{').count());
        self.depth = self.depth.saturating_sub(line.matches('}').count());
        if let Some(m) = self.in_mount
            && self.depth < m
        {
            self.in_mount = None;
        }
        if let Some(d) = self.impl_depth
            && self.depth < d
        {
            self.impl_depth = None;
        }
        if let Some(d) = self.in_test_mod
            && self.depth < d
        {
            self.in_test_mod = None;
        }
    }
}

/// `true` when `code` contains a `.route(` call that mounts a route - one with at least one argument
/// on this line or starting on the next. A bare `.route()` getter (like `governed.route()`) also
/// contains `.route(` but has `)` immediately after, so it is excluded: it reads a path, it does not
/// mount one. `.route_layer(` is not matched because `_layer` sits between `route` and `(`.
fn is_route_mount(code: &str) -> bool {
    let Some((_before, after)) = code.split_once(".route(") else {
        return false;
    };
    // `.route()` - a getter with no arguments - is not a mount. `.route(` at end of line, or
    // `.route(` followed by a non-`)` character, is a mount (the first argument is here or next line).
    !after.trim_start().starts_with(')')
}

/// The trimmed text of the first argument to `.route(` on this line, or `None` if the argument is
/// not complete on this line (the `(` is at end of line, or the argument spans past it). The
/// scanner respects string literals (a comma inside a `"/a,b"` path is not a separator) and
/// nested `()`/`[]`/`{}` (a comma inside a nested call's argument list is not a separator).
fn route_first_arg(code: &str) -> Option<&str> {
    let (_before, after_paren) = code.split_once(".route(")?;
    // If nothing follows the `(`, the argument is on a later line.
    let trimmed = after_paren.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('\n') {
        return None;
    }
    let (arg, _rest) = split_at_top_level_comma(trimmed)?;
    Some(arg.trim())
}

/// Split `code` at the first comma at the top level of the call - outside any string literal and
/// outside nested `()`/`[]`/`{}` - returning the text before it and the text after it. `None` when
/// no such comma is present on this line (the argument spans a later line). A string interior is
/// skipped with `\\` escapes honoured, so a comma inside `"/a,b"` is not mistaken for a separator.
fn split_at_top_level_comma(code: &str) -> Option<(&str, &str)> {
    let mut depth = 0_usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in code.char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '{' => depth = depth.saturating_add(1),
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some((code.get(..i)?, code.get(i.saturating_add(1)..)?)),
            _ => {}
        }
    }
    None
}

/// Everything on `line` from the first `//` onward, for judging code rather than comments.
fn strip_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

#[cfg(test)]
mod tests {
    use super::{MountSiteScan, route_first_arg};

    #[test]
    fn a_mount_outside_ungoverned_mount_is_refused() {
        // The shape `#758` M6 measured: a raw `.nest_service`/`.nest` of an ungoverned route with
        // no `Ungoverned::mount` in sight. This is the half that would redden that change.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        let r = Router::new().nest_service(\"/mcp\", svc);")
                .is_some(),
            "a stray nest_service outside the mechanism is refused"
        );
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        .nest(\"/mcp-someday\", subtree)").is_some(),
            "a stray nest outside the mechanism is refused"
        );
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        Router::new().route_service(\"/x\", svc)").is_some(),
            "a stray route_service outside the mechanism is refused"
        );
    }

    #[test]
    fn a_fallback_service_outside_mount_is_refused() {
        // `#758` round-2 review item 1: `.fallback_service` is the same class of mount primitive as
        // `.nest_service`/`.route_service` and was missing from the needle list entirely.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        Router::new().fallback_service(tower::service_fn(open))")
                .is_some(),
            "a stray fallback_service outside the mechanism is refused"
        );
    }

    #[test]
    fn a_wildcard_route_outside_mount_is_refused() {
        // `#758` round-2 review item 1: a top-level `.route("/rogue/{*rest}", …)` is outside
        // `every_route_governed` (which reads only the versioned document), outside `check_ungoverned`
        // (nothing recorded) and, before this needle, outside this gate too.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        .route(\"/rogue/{*rest}\", axum::routing::any(|| async { \"open\" }))")
                .is_some(),
            "a stray wildcard route outside the mechanism is refused"
        );
    }

    #[test]
    fn an_unrecorded_named_route_outside_mount_is_refused() {
        // `#1037`: a plain non-wildcard `.route("/rogue", …)` at the top level of `assemble` is
        // outside `Ungoverned::mount`, outside the governed subtree, and not in `ALLOWED_ROUTES`,
        // so it is refused. Before the `.route(` needle, this shape was held by review alone.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        .route(\"/rogue\", axum::routing::get(|| async { \"open\" }))")
                .is_some(),
            "an unrecorded named route outside the mechanism is refused"
        );
    }

    #[test]
    fn a_named_route_inside_a_test_module_passes() {
        // `#1037`: a `.route(` inside a `#[cfg(test)] mod tests` block is test code, not a shipped
        // route, so it passes. This is the shape `capability.rs` and `tls.rs` carry.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/capability.rs");
        let source = [
            "#[cfg(test)]",
            "mod tests {",
            "    fn a_cell() {",
            "        let app = axum::Router::new()",
            "            .route(\"/guarded\", axum::routing::get(|| async { StatusCode::OK }));",
            "    }",
            "}",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons.iter().all(Option::is_none),
            "a .route( inside a #[cfg(test)] mod tests block is test code: {reasons:?}"
        );
    }

    #[test]
    fn a_multi_line_route_inside_a_test_module_passes() {
        // `#1037`: the twin of the cell above for a `.route(` whose first argument starts on the
        // next line - the pending-argument arm judges the test-module exemption on its own.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/tls.rs");
        let source = [
            "#[cfg(test)]",
            "mod tests {",
            "    fn a_cell() {",
            "        let app = axum::Router::new()",
            "            .route(",
            "                \"/x\",",
            "                axum::routing::get(|| async { StatusCode::OK }),",
            "            );",
            "    }",
            "}",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons.iter().all(Option::is_none),
            "a multi-line .route( inside a #[cfg(test)] mod tests block is test code: {reasons:?}"
        );
    }

    #[test]
    fn a_named_route_in_a_tests_rs_file_passes() {
        // `#1037`: a `.route(` in a `tests.rs` file (an out-of-line test module declared by
        // `#[cfg(test)] mod tests;` in the parent) is test code, so it passes. This is the shape
        // `inbound/tests.rs` carries.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/inbound/tests.rs");
        assert!(
            scan.feed("        .route(\"/guarded\", get(|| async { StatusCode::NO_CONTENT }))")
                .is_none(),
            "a .route( in a tests.rs file is test code"
        );
    }

    #[test]
    fn a_recorded_production_route_passes() {
        // `#1037`: the two production `.route(` sites are in `ALLOWED_ROUTES`, so they pass even
        // though they are outside `Ungoverned::mount` and outside a test module.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        // `router.rs` mounts `.route(` with the first argument on the next line, like the real tree.
        let source = [
            "    let served = Router::new()",
            "        .route(",
            "            OPENAPI_JSON_PATH,",
            "            axum::routing::get(move || { let body = json.clone(); async move { body } }),",
            "        );",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons.iter().all(Option::is_none),
            "a recorded production .route( site passes: {reasons:?}"
        );

        let mut scan = MountSiteScan::new("crates/sutura-http/src/routes/protected_resource.rs");
        let source = [
            "    pub(crate) fn router(&self) -> Router {",
            "        Router::new()",
            "            .without_v07_checks()",
            "            .route(",
            "                &self.path,",
            "                get(move || async { body }),",
            "            )",
            "    }",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons.iter().all(Option::is_none),
            "a recorded production .route( site passes: {reasons:?}"
        );
    }

    #[test]
    fn a_route_in_the_wrong_file_with_the_right_key_is_refused() {
        // The allowlist keys on (file, first arg), not on the first arg alone: `OPENAPI_JSON_PATH`
        // in a file that is not `router.rs` is not recorded.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/routes/sneaky.rs");
        assert!(
            scan.feed("        .route(OPENAPI_JSON_PATH, axum::routing::get(|| async { \"open\" }))")
                .is_some(),
            "an allowlisted key in the wrong file is refused"
        );
    }

    #[test]
    fn a_second_impl_after_ungoverned_does_not_inherit_the_mount_exemption() {
        // The bug the review's NIT flagged: `in_impl` used to be set once and never cleared, so a
        // later `impl` block (or a `fn mount_other` inside `impl Ungoverned` itself) would inherit the
        // exemption by substring alone. Drive the scan past `impl Ungoverned`'s closing brace into an
        // unrelated `impl` and require its own nest_service to be refused.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        let source = [
            "impl Ungoverned {",
            "    pub(crate) fn mount<S>(path: &'static str, service: S) -> Self {",
            "        Self { router: Router::new().nest_service(path, service), path }",
            "    }",
            "}",
            "",
            "impl SomethingElse {",
            "    fn mount_other(svc: Svc) -> Router {",
            "        Router::new().nest_service(\"/sneaky\", svc)",
            "    }",
            "}",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons[8].is_some(),
            "a nest_service inside a later, unrelated impl must not inherit `Ungoverned::mount`'s exemption: {reasons:?}"
        );
    }

    #[test]
    fn a_mount_inside_the_mount_function_is_allowed() {
        // The one place an ungoverned route may be mounted: inside `Ungoverned::mount`, where the
        // type fuses it with its path. Drive the scanner line by line through that shape and require
        // the needle inside the body to pass.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        let source = [
            "impl Ungoverned {",
            "    pub(crate) fn mount<S>(path: &'static str, service: S) -> Self {",
            "        Self { router: Router::new().nest_service(path, service), path }",
            "    }",
            "}",
        ];
        for line in source {
            let reason = scan.feed(line);
            assert!(
                reason.is_none(),
                "the mount inside `Ungoverned::mount` is the one allowed site: {reason:?}"
            );
        }
    }

    #[test]
    fn a_multi_line_mount_signature_still_counts_as_inside() {
        // rustfmt puts a `where` clause between the signature and the body's `{`, so the body does
        // not open until a later line; `pending_mount` waits for that `{`.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        let source = [
            "impl Ungoverned {",
            "    pub(crate) fn mount<S>(path: &'static str) -> Self",
            "    where",
            "        S: Clone + Send + Sync + 'static,",
            "    {",
            "        Router::new().nest_service(path, svc)",
            "    }",
        ];
        for line in source {
            let reason = scan.feed(line);
            assert!(
                reason.is_none(),
                "a mount in a multi-line `Ungoverned::mount` signature is still the allowed site: {reason:?}"
            );
        }
    }

    #[test]
    fn a_governed_nest_is_allowed_anywhere() {
        // `.nest(API_V1_PREFIX, …)` is the governed subtree, held by `every_route_governed`; it is
        // allowed at the top of `assemble`, in `openapi.rs` and in `governed_routes` and must not
        // trip this half.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        assert!(
            scan.feed("        OpenApiRouter::new().nest(API_V1_PREFIX, routes::v1::openapi_router())")
                .is_none(),
            "the governed subtree's own nest is allowed"
        );
    }

    #[test]
    fn a_test_module_closing_does_not_inherit_to_later_code() {
        // A `.route(` after the `#[cfg(test)] mod tests` block closes is production code and must be
        // refused (unless allowlisted), so the test-module exemption does not leak past the block.
        let mut scan = MountSiteScan::new("crates/sutura-http/src/router.rs");
        let source = [
            "#[cfg(test)]",
            "mod tests {",
            "    .route(\"/in_test\", get(|| async {}))",
            "}",
            "",
            "let r = Router::new().route(\"/rogue\", get(|| async {}));",
        ];
        let mut reasons = Vec::new();
        for line in source {
            reasons.push(scan.feed(line));
        }
        assert!(
            reasons[5].is_some(),
            "a .route( after a test module closes is production code: {reasons:?}"
        );
    }

    #[test]
    fn route_first_arg_respects_commas_in_strings_and_nested_calls() {
        // M1 fix: `route_first_arg` must not split on a comma inside a string literal or a nested
        // call's argument list. The allowlist keys on the first argument, so truncating it at an
        // in-string comma would make a recorded key unmatchable.

        // (a) a comma inside a string-literal path argument.
        let line = r#"        .route("/a,b", get(|| async { "open" }))"#;
        assert_eq!(
            route_first_arg(line),
            Some(r#""/a,b""#),
            "a comma inside a string literal is not a separator"
        );

        // (b) a comma inside a nested call's string argument.
        let line = r#"        .route(get_path("x,y"), get(|| async {}))"#;
        assert_eq!(
            route_first_arg(line),
            Some(r#"get_path("x,y")"#),
            "a comma inside a nested call is not a separator"
        );

        // (c) an escaped double-quote inside the string - the comma after it is still in-string.
        let line = r#"        .route("/a\"b,c", get(|| async {}))"#;
        assert_eq!(
            route_first_arg(line),
            Some(r#""/a\"b,c""#),
            "an escaped quote does not end the string"
        );
    }
}
