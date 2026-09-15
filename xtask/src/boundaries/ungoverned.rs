//! The structural half of the ungoverned-route allowlist.
//!
//! `crate::router::{Ungoverned, check_ungoverned}` hold the allowlist at the TYPE: `Ungoverned::mount`
//! fuses a subtree with the path it was mounted at, so a merge and its record cannot drift. This half
//! is what makes "the only way to mount anything outside the governed subtree" true of the SOURCE
//! rather than of the recorder - the backstop that closes `#758` M6 (a subtree merged without being
//! recorded). It refuses any `.nest`/`.nest_service`/`.route_service` in `sutura-http` or `sutura-serve`
//! except inside `Ungoverned::mount`, and except the governed subtree's own `.nest(API_V1_PREFIX, …)`,
//! which `crate::router::governed_routes`/`every_route_governed` holds by a different mechanism.
//!
//! Limits, stated rather than hidden: it is line-oriented with a `//`-comment strip and no string
//! interior is blanked, and it tracks "inside `Ungoverned::mount`" by brace depth, so a mount spread
//! across lines whose needle is rewritten to land on a line outside the tracked depth would escape
//! it. The needles are specific enough that no current line hits that edge, and the type half is the
//! primary guard this scan backstops.

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
    in_impl: bool,
    /// Waiting for the `{` that opens `fn mount`'s body - its signature may span lines (a `where`
    /// clause), so "inside the mount" is only true once the body actually opens.
    pending_mount: bool,
    in_mount: Option<usize>,
}

impl MountSiteScan {
    const fn new() -> Self {
        Self {
            depth: 0,
            in_impl: false,
            pending_mount: false,
            in_mount: None,
        }
    }

    const fn in_mount(&self) -> bool {
        matches!(self.in_mount, Some(m) if self.depth >= m)
    }

    /// Judge one line (without its trailing newline), returning a reason if it mounts an ungoverned
    /// route outside the mechanism. State for the next line is derived from this line.
    fn feed(&mut self, line: &str) -> Option<&'static str> {
        let code = strip_line_comment(line);
        let needle = code.contains(".nest(") || code.contains(".nest_service(") || code.contains(".route_service(");
        let governed = code.contains(".nest(API_V1_PREFIX");
        let violation = needle && !governed && !self.in_mount();
        // State update for the NEXT line, from THIS line.
        if line.contains("impl Ungoverned") {
            self.in_impl = true;
        }
        if self.in_impl {
            if line.contains("fn mount") {
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
        violation.then_some("mounted outside the governed subtree and outside `Ungoverned::mount`")
    }
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
