#![forbid(unsafe_code)]
//! The byte bound on OKF catalog intake, held against the PUBLIC API only.
//!
//! **Why an integration test and not a `#[cfg(test)]` unit test.** `cargo xtask test-causality`
//! proves red-before-green by restoring the production file(s) a diff touched to `origin/main` and
//! re-running the new tests against that restored build. This crate's unit tests live in
//! `src/tests.rs`, wired into the build by `mod tests;` in `src/lib.rs` - the exact line the byte
//! bound's diff also touches. Restoring `lib.rs` to base reverts that line, so the split-out file
//! never gets referenced by any `mod` statement in the restored build and silently drops out of
//! it - a compiling, passing, but EMPTY measurement. `telekom/sutura#657` is the same trap in the
//! inbound key-refresh path, and `crates/sutura-catalog-local/tests/bounds.rs` documents it for
//! that crate's own two bounds. A file under `tests/` has no such wire: cargo discovers and
//! compiles every file here on its own, so it stays part of the build whichever half of the diff a
//! causality pass restores.
//!
//! **Why the refusal is asserted by rendered message rather than matching a variant.**
//! `OkfCatalogError::TooLarge` does not exist on `origin/main` - matching it by name would refuse
//! to COMPILE there, not fail at runtime, which is not red-before-green evidence (a compile error
//! confounds "this is refused" with "this cannot even be asked"). The fixture is instead a
//! complete, valid, otherwise-unbounded catalog: on `origin/main`, where the bound does not exist,
//! the same directory loads successfully. `OkfCatalog::load` is called through
//! [`sutura_domain::pinned::SemanticCatalog`], its own driving port, so this measures exactly what
//! a real deployment's boot path calls.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use sutura_catalog_okf::{OkfCatalog, OkfCatalogError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    fn test_name() -> SourceName {
        SourceName::parse("test").expect("a test name is a name")
    }

    fn version() -> DefinitionVersion {
        DefinitionVersion::parse("test-1").expect("a test version is a version")
    }

    /// A scratch directory of this test's own, cleared on the way in - matching the pattern this
    /// crate's own unit tests already use (`tempfile` is not a dependency of this workspace).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-bounds-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// A complete, valid, uniquely-named Table Schema descriptor - a real catalog element on its
    /// own, so a directory of these is a real catalog rather than a fixture that only looks like
    /// one to the one check under test.
    fn descriptor(unique: &str) -> String {
        format!("description: Bounds fixture model {unique}, one row per unit.\nfields:\n  - name: id\n")
    }

    /// A complete, valid descriptor whose TOTAL byte length on disk is exactly `total_len` -
    /// padded with a YAML comment at the top of the document. `deny_unknown_fields` refuses a key
    /// this adapter does not read, and the columns are parsed through validated name newtypes, so
    /// the only safe padding is a comment - which the YAML parser ignores and never surfaces as a
    /// field, so no per-field check fires for an unrelated reason.
    fn padded_descriptor(unique: &str, total_len: usize) -> String {
        let body = descriptor(unique);
        let prefix = "# ";
        let suffix = "\n";
        let padding_len = total_len
            .checked_sub(body.len() + prefix.len() + suffix.len())
            .expect("total_len must be large enough to hold the descriptor and its comment around the padding");
        format!("{prefix}{}{suffix}{body}", "a".repeat(padding_len))
    }

    /// The aggregate is a SUM, not a per-file limit, and the bound is a startup refusal naming
    /// bytes - the same two properties `sutura-catalog-local`'s own bound test pins for that
    /// crate. Two documents, each well under the cap on its own, whose total is over it by one:
    /// a mutation that checked each file's own size against the cap (rather than a running total)
    /// would never refuse this directory - neither file is individually oversized - so this is the
    /// cell that catches exactly that weakening, and it costs `MAX_CATALOG_BYTES + 1` bytes on disk
    /// rather than a 20 MiB file.
    #[test]
    fn a_directory_whose_documents_sum_past_the_byte_bound_is_refused() {
        const CAP: usize = 16 * 1024 * 1024;
        // Each file is almost but not quite the whole cap; their sum is CAP + 1, one byte over. The
        // `a` descriptor at the start is the tiny, valid one; `b_oversized` (no hyphen: the file
        // stem is the table name, and a hyphen is not a name) carries the bulk.
        const A_LEN: usize = 1024;
        const B_LEN: usize = CAP + 1 - A_LEN;
        let root = scratch("too-large-aggregate");
        std::fs::write(root.join("a.yaml"), padded_descriptor("a", A_LEN)).expect("a descriptor is writable");
        std::fs::write(root.join("b_oversized.yaml"), padded_descriptor("b", B_LEN)).expect("a descriptor is writable");

        let catalog = OkfCatalog::new(test_name(), root.clone(), version());
        let err = catalog.load().expect_err(
            "on origin/main these two descriptors load in full regardless of size - refusing \
             the directory is exactly the behaviour this test exists to require",
        );
        let message = err.to_string();
        assert!(
            message.contains(&CAP.to_string()),
            "the refusal should name the byte limit itself ({CAP}), not just the word 'bytes': {message}"
        );
        assert!(
            message.contains("b_oversized"),
            "the refusal should name the document whose read stopped it, not only the catalog root: {message}"
        );

        drop(std::fs::remove_dir_all(&root));
    }

    type Attempt = std::cell::RefCell<Option<Result<sutura_domain::pinned::PinnedDefinitions, OkfCatalogError>>>;
    /// Creates a FIFO at `path` - `std` has no `mkfifo`, and the swap under test is a FIFO at
    /// the document's own name, which is what `O_NONBLOCK` at open exists to refuse. The
    /// system's `mkfifo` is used because a FIFO cannot be created from safe `std` alone.
    fn make_fifo(path: &std::path::Path) {
        let status = std::process::Command::new("mkfifo").arg(path).status().expect("mkfifo runs");
        assert!(status.success(), "mkfifo succeeded: {status}");
    }

    /// The symlink half of the swap the one-open fix exists for - the okf half. The walk
    /// lists descriptors by their ENTRY type, so a symlink is skipped before any reader
    /// runs; the swap reaches the reader only inside the window between the walk's listing
    /// and the open of each listed path. Each attempt lands the swap there when the lead
    /// descriptor's read begins; on base `File::open` FOLLOWS the swap and the catalog
    /// loads with content the walk never listed, so no attempt is ever refused and the
    /// retries run out - the red. On the fixed reader the open carries `O_NOFOLLOW` and
    /// the swap is refused with the open's `ELOOP`.
    ///
    /// **The limit, stated next to the claim:** the window is sampled per attempt and an
    /// attempt whose trigger fires after `b`'s open is a plain successful load - which is
    /// why there are attempts at all. Red on base is exhaustion across them, not a hang.
    #[test]
    fn a_descriptor_swapped_in_the_read_window_is_refused_not_followed() {
        const MAX_ATTEMPTS: usize = 10;

        let root = scratch("read-window-symlink");
        std::fs::write(root.join("a.yaml"), padded_descriptor("lead", 12 << 20)).expect("a descriptor is writable");
        let outside = scratch("read-window-symlink-target");
        std::fs::write(outside.join("outside.yaml"), descriptor("outside")).expect("an outside descriptor is writable");
        let link = root.join(".swap-object");
        std::os::unix::fs::symlink(outside.join("outside.yaml"), &link).expect("the prepared symlink is creatable");
        let catalog = OkfCatalog::new(test_name(), root.clone(), version());
        let document = root.join("b.yaml");
        let saved = root.join(".swap-saved");
        std::fs::write(&document, descriptor("b")).expect("a descriptor is writable");
        std::fs::write(&saved, descriptor("b")).expect("a descriptor is writable");
        for _ in 0..MAX_ATTEMPTS {
            let armed = std::sync::atomic::AtomicBool::new(false);
            let done = std::sync::atomic::AtomicBool::new(false);
            let outcome: Attempt = std::cell::RefCell::new(None);
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    armed.store(true, std::sync::atomic::Ordering::Release);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    drop(std::fs::rename(&link, &document));
                    while !done.load(std::sync::atomic::Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    drop(std::fs::rename(&document, &link));
                    drop(std::fs::rename(&saved, &document));
                    drop(std::fs::write(
                        &saved,
                        std::fs::read(&document).expect("the restored descriptor reads"),
                    ));
                });
                while !armed.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::yield_now();
                }
                *outcome.borrow_mut() = Some(catalog.load());
                done.store(true, std::sync::atomic::Ordering::Release);
            });
            let attempted = outcome.into_inner().expect("the scoped thread recorded the load's outcome");
            if let Err(err) = attempted {
                let message = err.to_string();
                if message.contains("Too many levels of symbolic links") {
                    drop(std::fs::remove_dir_all(&root));
                    return;
                }
                panic!("refused, but not by the expected refusal - a later failure fired first: {message}");
            }
        }
        panic!(
            "in {MAX_ATTEMPTS} attempts, a descriptor swapped in the read window was never \
             refused - the open followed the swap, or the swap never landed in a window; \
             both is the failure this cell exists to catch"
        );
    }
    /// The FIFO half: the swap lands between the walk and the open, and on base
    /// `File::open` of a FIFO with no writer BLOCKS - a load that never returns, so the red
    /// here is base's hang (bounded on CI by the test runner's per-test timeout). On the
    /// fixed reader the open carries `O_NONBLOCK`, returns at once, and the FIFO is refused
    /// on the handle's own regular-file check - a refusal base never produces.
    #[test]
    fn a_descriptor_swapped_for_a_fifo_in_the_read_window_is_refused_not_blocked() {
        const MAX_ATTEMPTS: usize = 10;

        let root = scratch("read-window-fifo");
        std::fs::write(root.join("a.yaml"), padded_descriptor("lead", 12 << 20)).expect("a descriptor is writable");
        let fifo = root.join(".swap-object");
        make_fifo(&fifo);
        let catalog = OkfCatalog::new(test_name(), root.clone(), version());
        let document = root.join("b.yaml");
        let saved = root.join(".swap-saved");
        std::fs::write(&document, descriptor("b")).expect("a descriptor is writable");
        std::fs::write(&saved, descriptor("b")).expect("a descriptor is writable");
        for _ in 0..MAX_ATTEMPTS {
            let armed = std::sync::atomic::AtomicBool::new(false);
            let done = std::sync::atomic::AtomicBool::new(false);
            let outcome: Attempt = std::cell::RefCell::new(None);
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    armed.store(true, std::sync::atomic::Ordering::Release);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    drop(std::fs::rename(&fifo, &document));
                    while !done.load(std::sync::atomic::Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    drop(std::fs::rename(&document, &fifo));
                    drop(std::fs::rename(&saved, &document));
                    drop(std::fs::write(
                        &saved,
                        std::fs::read(&document).expect("the restored descriptor reads"),
                    ));
                });
                while !armed.load(std::sync::atomic::Ordering::Acquire) {
                    std::thread::yield_now();
                }
                *outcome.borrow_mut() = Some(catalog.load());
                done.store(true, std::sync::atomic::Ordering::Release);
            });
            let attempted = outcome.into_inner().expect("the scoped thread recorded the load's outcome");
            if let Err(err) = attempted {
                let message = err.to_string();
                if message.contains("not a regular file") {
                    drop(std::fs::remove_dir_all(&root));
                    return;
                }
                panic!("refused, but not by the expected refusal - a later failure fired first: {message}");
            }
        }
        panic!(
            "in {MAX_ATTEMPTS} attempts, a descriptor swapped in the read window was never \
             refused - the open followed the swap, or the swap never landed in a window; \
             both is the failure this cell exists to catch"
        );
    }
}
