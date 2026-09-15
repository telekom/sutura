//! The structural half of the ungoverned-route allowlist.
//!
//! `crate::router::{Ungoverned, check_ungoverned}` hold the allowlist at the TYPE: `Ungoverned::mount`
//! fuses a subtree with the path it was mounted at, and no other method on that type hands back a
//! bare, re-fusable `Router` - see that type's own doc for what holds. This half is what makes "the
//! only way to mount anything outside the governed subtree" true of the SOURCE rather than of a
//! caller who never reaches for `Ungoverned` at all - the backstop that closes `#758` M6 (a subtree
//! merged with no `Ungoverned` in sight). It refuses any
//! `.nest`/`.nest_service`/`.route_service`/`.fallback_service`, and a wildcard `.route("...{*...",
//! …)`, in `sutura-http` or `sutura-serve` except inside `Ungoverned::mount`, and except the governed
//! subtree's own `.nest(API_V1_PREFIX, …)`, which `crate::router::governed_routes`/
//! `every_route_governed` holds by a different mechanism.
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
//! Limits, stated rather than hidden: it is line-oriented with a `//`-comment strip and no string
//! interior is blanked, and it tracks "inside `Ungoverned::mount`" and "inside `impl Ungoverned`" by
//! brace depth, so a mount spread across lines whose needle is rewritten to land on a line outside
//! the tracked depth would escape it, and the wildcard `.route` needle only fires when the path
//! literal is on the SAME line as `.route(`. The needles are specific enough that no current line
//! hits either edge, and the type half is the primary guard this scan backstops.

use crate::Verdict;

/// Runs the scan over both serving crates, and refuses a mount primitive anywhere but the mechanism.
pub(crate) fn check() -> Verdict {
    let Some(root) = crate::repo::root() else {
        eprintln!("xtask check-boundaries: could not find the workspace root");
        return Verdict::Fail;
    };
    let mut problems: Vec<String> = Vec::new();
    let mut files = 0usize;
    for base in ["crates/sutura-http/src", "crates/sutura-serve/src"] {
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
                    let mut scan = MountSiteScan::new();
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
        eprintln!("Every `.nest`/`.nest_service`/`.route_service` in `sutura-http` or `sutura-serve` must be either");
        eprintln!("the governed subtree's own `.nest(API_V1_PREFIX, ...)` or inside `crate::router::Ungoverned::mount`.");
        return Verdict::Fail;
    }
    println!("xtask check-boundaries: ok - every ungoverned mount lives inside `Ungoverned::mount` ({files} source file(s))");
    Verdict::Pass
}

/// The stateful line scanner behind [`check`].
///
/// Tracks brace depth and whether the walk is inside `Ungoverned::mount`'s body, so a mount
/// primitive is judged against the mechanism rather than against a file name.
struct MountSiteScan {
    depth: usize,
    /// The depth `impl Ungoverned`'s body opens at, cleared once the walk leaves it - so a later
    /// `impl` (or a `fn mount_other` in this one) is never mistaken for the exempted site.
    impl_depth: Option<usize>,
    /// Waiting for the `{` that opens `fn mount`'s body - its signature may span lines (a `where`
    /// clause), so "inside the mount" is only true once the body actually opens.
    pending_mount: bool,
    in_mount: Option<usize>,
}

impl MountSiteScan {
    const fn new() -> Self {
        Self {
            depth: 0,
            impl_depth: None,
            pending_mount: false,
            in_mount: None,
        }
    }

    const fn in_mount(&self) -> bool {
        matches!(self.in_mount, Some(m) if self.depth >= m)
    }

    const fn in_impl(&self) -> bool {
        matches!(self.impl_depth, Some(d) if self.depth >= d)
    }

    /// Judge one line (without its trailing newline), returning a reason if it mounts an ungoverned
    /// route outside the mechanism. State for the next line is derived from this line.
    fn feed(&mut self, line: &str) -> Option<&'static str> {
        let code = strip_line_comment(line);
        let needle = code.contains(".nest(")
            || code.contains(".nest_service(")
            || code.contains(".route_service(")
            || code.contains(".fallback_service(")
            || is_wildcard_route(code);
        let governed = code.contains(".nest(API_V1_PREFIX");
        let violation = needle && !governed && !self.in_mount();
        // State update for the NEXT line, from THIS line. `in_impl` is read BEFORE this line's own
        // `impl Ungoverned` (if any) opens it, so the line naming the impl is not itself "in" it.
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
                    self.pending_mount = false;
                } else {
                    self.pending_mount = true;
                }
            } else if self.pending_mount && line.contains('{') {
                self.in_mount = Some(self.depth + 1);
                self.pending_mount = false;
            }
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
        violation.then_some("mounted outside the governed subtree and outside `Ungoverned::mount`")
    }
}

/// `true` when `code` opens a route on a wildcard path segment (`{*name}`) - the shape `axum`
/// accepts anywhere a router is built, not only under the versioned prefix `every_route_governed`
/// reads, so a bare `.route("/rogue/{*rest}", …)` at the top level of `assemble` is otherwise
/// invisible to every mechanism this crate holds. Limit: the path literal must be on the SAME line
/// as `.route(` - one built across a line break (rustfmt does not do this for a short literal, but
/// nothing stops a hand-written one) escapes this needle, the same class of gap the module doc
/// states for `Ungoverned::mount` itself.
fn is_wildcard_route(code: &str) -> bool {
    code.contains(".route(") && code.contains("{*")
}

/// Everything on `line` from the first `//` onward, for judging code rather than comments.
fn strip_line_comment(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

#[cfg(test)]
mod tests {
    use super::MountSiteScan;

    #[test]
    fn a_mount_outside_ungoverned_mount_is_refused() {
        // The shape `#758` M6 measured: a raw `.nest_service`/`.nest` of an ungoverned route with
        // no `Ungoverned::mount` in sight. This is the half that would redden that change.
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        let r = Router::new().nest_service(\"/mcp\", svc);")
                .is_some(),
            "a stray nest_service outside the mechanism is refused"
        );
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        .nest(\"/mcp-someday\", subtree)").is_some(),
            "a stray nest outside the mechanism is refused"
        );
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        Router::new().route_service(\"/x\", svc)").is_some(),
            "a stray route_service outside the mechanism is refused"
        );
    }

    #[test]
    fn a_fallback_service_outside_mount_is_refused() {
        // `#758` round-2 review item 1: `.fallback_service` is the same class of mount primitive as
        // `.nest_service`/`.route_service` and was missing from the needle list entirely.
        let mut scan = MountSiteScan::new();
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
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        .route(\"/rogue/{*rest}\", axum::routing::any(|| async { \"open\" }))")
                .is_some(),
            "a stray wildcard route outside the mechanism is refused"
        );
    }

    #[test]
    fn an_ordinary_named_route_is_allowed_anywhere() {
        // The wildcard needle must not fire on the ordinary named routes this codebase mounts by the
        // hundred - only a literal `{*` path segment is the shape it is looking for.
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        .route(\"/health\", axum::routing::get(health))").is_none(),
            "a route with no wildcard segment is not a mount primitive this half judges"
        );
    }

    #[test]
    fn a_second_impl_after_ungoverned_does_not_inherit_the_mount_exemption() {
        // The bug the review's NIT flagged: `in_impl` used to be set once and never cleared, so a
        // later `impl` block (or a `fn mount_other` inside `impl Ungoverned` itself) would inherit the
        // exemption by substring alone. Drive the scan past `impl Ungoverned`'s closing brace into an
        // unrelated `impl` and require its own nest_service to be refused.
        let mut scan = MountSiteScan::new();
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
        let mut scan = MountSiteScan::new();
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
        let mut scan = MountSiteScan::new();
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
        let mut scan = MountSiteScan::new();
        assert!(
            scan.feed("        OpenApiRouter::new().nest(API_V1_PREFIX, routes::v1::openapi_router())")
                .is_none(),
            "the governed subtree's own nest is allowed"
        );
    }
}
