#![forbid(unsafe_code)]
//! Every reachable `OkfCatalogError` refusal `src/tests.rs` does not provoke, each driven through
//! the public [`SemanticCatalog::load`] port over a scratch directory of real Table Schema
//! descriptors and asserted by variant, the contract a caller matches on.
//!
//! Not provoked: `UncheckableKnowledge` (this adapter supplies `KnowledgeInput::none()`) and
//! `Digest` (no public-port fixture reaches a pinning digest failure). `Unnamed` is provoked
//! on Linux only - a file name that is not UTF-8, which not every filesystem can hold. `Io` is
//! provoked on both arms: its read arm (a non-UTF-8 document) and its walk arm (an unreadable
//! subdirectory). `TooLarge`, `Open`, and `NotARegularFile` are provoked here; `tests/bounds.rs`
//! also tests `TooLarge` as an integration bound.
//!
//! Wrapped in `#[cfg(test)] mod tests` so `allow-expect-in-tests` reaches the helpers, the shape
//! `tests/bounds.rs` uses.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_catalog_okf::{InvalidPrimaryKeyShape, OkfCatalog, OkfCatalogError};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};

    const VALID: &str = "description: A model.\nfields:\n  - name: id\n";

    fn load(root: &Path) -> Option<OkfCatalogError> {
        OkfCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.to_path_buf(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        )
        .load()
        .err()
    }

    /// A scratch directory of this test's own, cleared on the way in.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sutura-catalog-okf-refusals-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("a scratch directory is creatable");
        dir
    }

    /// Loads a directory holding one `document` as `file` and returns its refusal, removing the
    /// directory.
    fn refused(name: &str, file: &str, document: &[u8]) -> Option<OkfCatalogError> {
        let root = scratch(name);
        std::fs::write(root.join(file), document).expect("a descriptor is writable");
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        outcome
    }

    #[test]
    fn a_non_directory_root_is_refused() {
        let root = scratch("file-root");
        let file = root.join("orders.yaml");
        std::fs::write(&file, VALID).expect("a descriptor is writable");
        let outcome = load(&file);
        drop(std::fs::remove_dir_all(&root));
        assert!(matches!(outcome, Some(OkfCatalogError::NotADirectory { .. })), "{outcome:?}");
    }

    #[test]
    fn a_document_that_is_not_utf8_is_refused_as_io() {
        let outcome = refused("non-utf8", "orders.yaml", b"\xff\xfe not utf8");
        assert!(matches!(outcome, Some(OkfCatalogError::Io { .. })), "{outcome:?}");
    }

    #[test]
    fn a_file_stem_that_is_not_a_table_name_is_refused() {
        let outcome = refused("bad-stem", "1orders.yaml", VALID.as_bytes());
        assert!(matches!(outcome, Some(OkfCatalogError::InvalidName { .. })), "{outcome:?}");
    }

    #[test]
    fn a_field_name_that_is_not_a_column_name_is_refused() {
        let outcome = refused(
            "bad-column",
            "orders.yaml",
            b"description: A model.\nfields:\n  - name: '1bad'\n",
        );
        assert!(matches!(outcome, Some(OkfCatalogError::InvalidColumn { .. })), "{outcome:?}");
    }

    #[test]
    fn an_okf_model_description_with_a_control_character_is_refused() {
        let outcome = refused(
            "ctrl-model",
            "orders.yaml",
            b"description: \"A model.\\rmore\"\nfields:\n  - name: id\n",
        );
        assert!(
            matches!(outcome, Some(OkfCatalogError::InvalidDescription { .. })),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_field_description_with_a_control_character_is_refused() {
        let descriptor = b"description: A model.\nfields:\n  - name: id\n    description: \"A field.\\rmore\"\n";
        let outcome = refused("ctrl-column", "orders.yaml", descriptor);
        assert!(
            matches!(outcome, Some(OkfCatalogError::InvalidColumnDescription { ref column, .. }) if column.as_str() == "id"),
            "{outcome:?}"
        );
    }

    #[test]
    fn a_directory_with_more_than_max_documents_is_refused() {
        let root = scratch("too-many");
        for i in 0..=sutura_bounded_read::MAX_CATALOG_DOCUMENTS {
            std::fs::write(root.join(format!("m{i}.yaml")), VALID).expect("a descriptor is writable");
        }
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Some(OkfCatalogError::TooManyDocuments { .. })),
            "{outcome:?}"
        );
    }

    /// A string `primaryKey` passes the shape check and fails the column-name parse -
    /// `src/tests.rs` provokes only the `NotAStringOrList` arm.
    #[test]
    fn a_primary_key_string_that_is_not_a_column_name_is_refused() {
        let outcome = refused(
            "bad-pk-name",
            "orders.yaml",
            b"description: A model.\nprimaryKey: '1bad'\nfields:\n  - name: id\n",
        );
        assert!(
            matches!(
                outcome,
                Some(OkfCatalogError::InvalidPrimaryKey {
                    cause: InvalidPrimaryKeyShape::Column { .. },
                    ..
                })
            ),
            "{outcome:?}"
        );
    }

    /// `Io` on the walk arm: `read_dir` on a subdirectory the runner cannot read fails, and the
    /// walk surfaces it as `OkfCatalogError::Io`. The root itself reads fine - only the child
    /// directory is unreadable - so the refusal is the walk's, not the root's `NotADirectory`.
    /// Holds for an unprivileged runner only: a root user reads a mode-000 directory.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_subdirectory_is_refused_as_io_through_the_walk() {
        use std::os::unix::fs::PermissionsExt as _;
        let root = scratch("unreadable-subdir");
        let subdir = root.join("subdir");
        std::fs::create_dir_all(&subdir).expect("a subdirectory is creatable");
        std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o000)).expect("permissions are settable");
        let outcome = load(&root);
        drop(std::fs::set_permissions(&subdir, std::fs::Permissions::from_mode(0o755)));
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(outcome, Some(OkfCatalogError::Io { ref path, .. }) if *path == subdir),
            "{outcome:?}"
        );
    }

    /// `TooManyEntries` bounds the tree, not the documents: a wide directory of skipped non-document
    /// entries is refused once the walk visits more than `MAX_CATALOG_ENTRIES` of them. The document
    /// cap never sees these - none of them is a `yaml`/`yml` file - so this is the entry cap's own
    /// refusal, mapped into this adapter's variant.
    #[test]
    fn a_directory_with_more_than_max_entries_is_refused() {
        let root = scratch("too-many-entries");
        for i in 0..=sutura_bounded_read::MAX_CATALOG_ENTRIES {
            std::fs::write(root.join(format!("entry-{i}.txt")), "not a document").expect("a non-document file is writable");
        }
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(
                outcome,
                Some(OkfCatalogError::TooManyEntries { found, limit, .. })
                    if found == sutura_bounded_read::MAX_CATALOG_ENTRIES + 1
                        && limit == sutura_bounded_read::MAX_CATALOG_ENTRIES
            ),
            "{outcome:?}"
        );
    }

    /// `TooLarge`: two descriptors whose bytes sum one past `MAX_CATALOG_BYTES`. The bound is on the
    /// read itself, enforced as a running total, so neither file is individually oversized - a
    /// mutation that checked each file's own size against the cap rather than the running total
    /// would never refuse this directory. `document` names the file whose read crossed the cap.
    #[test]
    fn a_directory_whose_documents_sum_past_the_byte_bound_is_refused_as_too_large() {
        const CAP: u64 = sutura_bounded_read::MAX_CATALOG_BYTES;
        // `a` is a tiny valid descriptor; `b_oversized` carries the bulk (no hyphen: the stem is the
        // table name, and a hyphen is not a name). Their sum is CAP + 1, one byte over. The padding
        // is a YAML comment at the top of `b`, which the parser ignores - the only safe padding,
        // because `deny_unknown_fields` refuses a key the adapter does not read.
        let root = scratch("too-large-variant");
        let a = "description: A small model.\nfields:\n  - name: id\n";
        let a_len = a.len() as u64;
        let target_b = CAP + 1 - a_len;
        let b_body = "description: The bulk model.\nfields:\n  - name: id\n";
        let prefix = "# ";
        let suffix = "\n";
        let padding = usize::try_from(target_b)
            .expect("target_b fits in usize on this platform")
            .saturating_sub(b_body.len() + prefix.len() + suffix.len());
        let b = format!("{prefix}{}{suffix}{b_body}", "a".repeat(padding));
        assert_eq!(
            b.len() as u64,
            target_b,
            "the bulk descriptor is exactly the bytes the cap needs"
        );
        std::fs::write(root.join("a.yaml"), a).expect("a descriptor is writable");
        std::fs::write(root.join("b_oversized.yaml"), b.as_bytes()).expect("a descriptor is writable");
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(
            matches!(
                outcome,
                Some(OkfCatalogError::TooLarge { ref document, found, limit, .. })
                    if document.file_name().is_some_and(|n| n == "b_oversized.yaml")
                        && limit == CAP
                        && found > limit
            ),
            "{outcome:?}"
        );
    }

    fn make_fifo(path: &std::path::Path) {
        let status = std::process::Command::new("mkfifo").arg(path).status().expect("mkfifo runs");
        assert!(status.success(), "mkfifo succeeded: {status}");
    }

    type Attempt = std::cell::RefCell<Option<Result<sutura_domain::pinned::PinnedDefinitions, OkfCatalogError>>>;

    /// A valid descriptor padded to `total_len` bytes via a leading YAML comment line - never
    /// parsed as a value, so it cannot trip `MAX_DESCRIPTION_BYTES`. Sorted first (`a.yaml`), its
    /// real disk read widens the read-window swap tests' race from microseconds to milliseconds,
    /// the same technique `sutura-catalog-local/tests/bounds.rs`'s own swap tests use.
    fn padded_lead(total_len: usize) -> String {
        let prefix = "# ";
        let suffix = "\n";
        let padding_len = total_len
            .checked_sub(prefix.len() + suffix.len() + VALID.len())
            .expect("total_len must be large enough to hold the padding comment and the descriptor body");
        format!("{prefix}{}{suffix}{VALID}", "a".repeat(padding_len))
    }

    /// `Open` via a symlink swap in the read window: the variant this adapter maps `ReadError::Open`
    /// to, asserted by `matches!` rather than the rendered `ELOOP` text `tests/bounds.rs` checks.
    ///
    /// **The limit, stated next to the claim:** the window is sampled per attempt and an attempt
    /// whose swap fires after `b.yaml`'s open is a plain successful load - which is why there are
    /// attempts at all. A lead document (`a.yaml`, sorted first) pads the walk's real disk I/O to
    /// milliseconds so the 20ms-delayed swap reliably lands before the target's own open.
    #[cfg(unix)]
    #[test]
    fn a_descriptor_swapped_in_the_read_window_is_refused_as_open() {
        const MAX_ATTEMPTS: usize = 25;
        let root = scratch("open-window-symlink");
        std::fs::write(root.join("a.yaml"), padded_lead(12 << 20)).expect("the lead descriptor is writable");
        let outside = scratch("open-window-symlink-target");
        std::fs::write(outside.join("outside.yaml"), VALID).expect("an outside descriptor is writable");
        let link = root.join(".swap-object");
        std::os::unix::fs::symlink(outside.join("outside.yaml"), &link).expect("the prepared symlink is creatable");
        let catalog = OkfCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.clone(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        );
        let document = root.join("b.yaml");
        let saved = root.join(".swap-saved");
        std::fs::write(&document, VALID).expect("a descriptor is writable");
        std::fs::write(&saved, VALID).expect("a descriptor is writable");
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
                assert!(
                    matches!(err, OkfCatalogError::Open { .. }),
                    "refused, but not by the open refusal: {err:?}"
                );
                drop(std::fs::remove_dir_all(&root));
                drop(std::fs::remove_dir_all(&outside));
                return;
            }
        }
        drop(std::fs::remove_dir_all(&root));
        drop(std::fs::remove_dir_all(&outside));
        panic!(
            "in {MAX_ATTEMPTS} attempts, a descriptor swapped in the read window was never refused \
             as Open - the open followed the swap, or the swap never landed in a window"
        );
    }

    /// `NotARegularFile`: a FIFO swapped into a descriptor's path in the walk→read window opens
    /// under `O_NONBLOCK` (so the open returns rather than blocks) and is then refused by the
    /// handle's regular-file check - `OkfCatalogError::NotARegularFile`, the variant the message
    /// test in `tests/bounds.rs` checks as "not a regular file" text.
    ///
    /// **The limit, stated next to the claim:** same sampled window as the `Open` cell above; a
    /// lead document (`a.yaml`) widens it to milliseconds for the same reason.
    #[cfg(unix)]
    #[test]
    fn a_descriptor_swapped_for_a_fifo_in_the_read_window_is_refused_as_not_a_regular_file() {
        const MAX_ATTEMPTS: usize = 25;
        let root = scratch("not-regular-window-fifo");
        std::fs::write(root.join("a.yaml"), padded_lead(12 << 20)).expect("the lead descriptor is writable");
        let fifo = root.join(".swap-object");
        make_fifo(&fifo);
        let catalog = OkfCatalog::new(
            SourceName::parse("test").expect("a test name is a name"),
            root.clone(),
            DefinitionVersion::parse("test-1").expect("a test version is a version"),
        );
        let document = root.join("b.yaml");
        let saved = root.join(".swap-saved");
        std::fs::write(&document, VALID).expect("a descriptor is writable");
        std::fs::write(&saved, VALID).expect("a descriptor is writable");
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
                assert!(
                    matches!(err, OkfCatalogError::NotARegularFile { .. }),
                    "refused, but not by the not-a-regular-file refusal: {err:?}"
                );
                drop(std::fs::remove_dir_all(&root));
                return;
            }
        }
        drop(std::fs::remove_dir_all(&root));
        panic!(
            "in {MAX_ATTEMPTS} attempts, a descriptor swapped for a FIFO in the read window was \
             never refused as NotARegularFile - the open never reached the regular-file check, or \
             the swap never landed in a window"
        );
    }

    /// `Unnamed`: a descriptor whose file stem is not valid UTF-8, so `file_stem().to_str()` is
    /// `None`. Only a filesystem that holds non-UTF-8 names can build this fixture - APFS refuses
    /// the bytes at creation, ext4/tmpfs do not - so the test is Linux-only. The stem is a bare
    /// high byte the name parser would also refuse, but the UTF-8 check fires first and earlier in
    /// `descriptor_to_model`, so the refusal is `Unnamed`, not `InvalidName`.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_descriptor_whose_file_stem_is_not_utf8_is_refused_as_unnamed() {
        use std::os::unix::ffi::OsStrExt as _;
        let root = scratch("non-utf8-stem");
        // A single 0x80 byte is not a valid UTF-8 lead byte, so `file_stem().to_str()` is `None`.
        let name = std::ffi::OsStr::from_bytes(b"\x80.yaml");
        std::fs::write(root.join(name), VALID.as_bytes()).expect("a non-UTF-8 descriptor is writable");
        let outcome = load(&root);
        drop(std::fs::remove_dir_all(&root));
        assert!(matches!(outcome, Some(OkfCatalogError::Unnamed { .. })), "{outcome:?}");
    }
}
