#![forbid(unsafe_code)]
//! `sutura import wren <dir> <out>` end to end, over the SYNTHETIC fixture at
//! `tests/fixtures/wren-import`: a golden snapshot of every generated document plus the refusal
//! report, and a proof that the converted catalog loads and answers one question.
//!
//! **Spawns the compiled binary, and that departs from `tests/example.rs`'s own rule** of driving
//! libraries. The conversion itself is a library call now (`sutura_catalog_wren::import`, whose own
//! unit cells cover the recognisers and the refusal kinds), but `sutura-cli` has no lib target, so
//! the command's argument parsing and the two lines it prints are reachable only through the
//! binary. This is the only place the actual argv, exit code and stdout the command prints are
//! exercised at all. The load-and-answer half below spawns nothing; it drives
//! `sutura_catalog_local` and `sutura_app` directly, exactly as `tests/example.rs` does, over the
//! directory the binary just wrote.

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::{fs, time};

    use sutura_catalog_local::LocalCatalog;
    use sutura_domain::identity::{PrincipalChain, RequestContext, Subject};
    use sutura_domain::model::SourceName;
    use sutura_domain::pinned::{DefinitionVersion, SemanticCatalog as _};
    use sutura_domain::query::{Query, ToolOutcome};
    use sutura_domain::source::{AcknowledgementReason, SharedIdentityDeclared, SourcePosture};
    use sutura_domain::warehouse::Value;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wren-import")
    }

    /// A destination this test's process owns alone - not `tempfile`, which is not a dependency of
    /// this workspace; the standard temp directory plus the process id and a nanosecond timestamp
    /// is unique enough for one test run and is removed again at the end either way.
    fn scratch_out_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "sutura-import-wren-test-{}-{}",
            std::process::id(),
            time::SystemTime::now()
                .duration_since(time::UNIX_EPOCH)
                .expect("the clock is after 1970")
                .as_nanos()
        ))
    }

    /// Every file under `dir`, sorted by relative path.
    fn files_sorted(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        collect(dir, &mut found);
        found.sort();
        found
    }

    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()))
            .map(|entry| entry.expect("a directory entry is readable").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                collect(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    /// A second run into a directory an earlier run filled would leave that run's documents beside
    /// this one's, so a non-empty `<out>` is refused before anything is written - and nothing the
    /// operator already had there is touched.
    #[test]
    fn a_non_empty_out_dir_is_refused_by_name_and_left_untouched() {
        let out_dir = scratch_out_dir();
        let stale = out_dir.join("models/stale_model.md");
        fs::create_dir_all(stale.parent().expect("a file under models/ has a parent")).expect("the scratch dir is writable");
        fs::write(&stale, "an earlier run's document\n").expect("the scratch dir is writable");

        let output = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("import")
            .arg("wren")
            .arg(fixture_root())
            .arg(&out_dir)
            .output()
            .expect("the sutura binary runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        let listed = files_sorted(&out_dir);
        drop(fs::remove_dir_all(&out_dir));

        assert!(
            !output.status.success(),
            "a non-empty out dir was written into\nstderr: {stderr}"
        );
        assert!(
            stderr.contains(&format!("{} is not empty", out_dir.display())),
            "the refusal must name the directory: {stderr}"
        );
        assert_eq!(
            listed,
            vec![stale],
            "nothing may be written beside, or instead of, what was there"
        );
    }

    #[test]
    fn the_synthetic_fixture_converts_to_a_golden_snapshot_and_the_result_loads_and_answers() {
        let out_dir = scratch_out_dir();
        let cleanup = || drop(fs::remove_dir_all(&out_dir));
        cleanup();

        let output = Command::new(env!("CARGO_BIN_EXE_sutura"))
            .arg("import")
            .arg("wren")
            .arg(fixture_root())
            .arg(&out_dir)
            .output()
            .expect("the sutura binary runs");
        assert!(
            output.status.success(),
            "import wren exited {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        // The golden snapshot: every generated document plus the report, one file per section,
        // sorted so the snapshot is a function of the output tree rather than of readdir order.
        let mut snapshot = String::new();
        for path in files_sorted(&out_dir) {
            let relative = path.strip_prefix(&out_dir).expect("every walked path is under out_dir");
            let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} is readable: {e}", path.display()));
            writeln!(snapshot, "=== {} ===", relative.display()).expect("writing to a String cannot fail");
            snapshot.push_str(&text);
            if !text.ends_with('\n') {
                snapshot.push('\n');
            }
        }
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.bind(|| {
            insta::assert_snapshot!("import_wren_output", snapshot);
        });

        // The load-and-answer half: the converted catalog loads through the same reader the
        // `catalog`/`query` commands use, with the fixture's own CSVs attached, and answers the
        // fixture's own question.
        let version = DefinitionVersion::parse("wren-import-test").expect("a fixed literal is a version");
        let name = SourceName::parse("wren").expect("the importer's own default source name is a name");
        let catalog = LocalCatalog::new(name, out_dir.clone(), version);
        let pinned = catalog
            .load()
            .unwrap_or_else(|e| panic!("the converted catalog does not load: {e}"));

        let sources = sutura_app::sources(&pinned);
        let [source] = sources.as_slice() else {
            panic!("the fixture is single-source, and this one names {} systems", sources.len());
        };
        let posture = SourcePosture::SharedServiceUser {
            declared: SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a test fixture read by one process, as that process's own identity")
                    .expect("the fixture reason is a reason"),
            ),
        };
        let ceiling = sutura_config::WorkingSetCeiling::parse(
            sutura_config::WorkingSetCeiling::DEFAULT_BYTES,
            sutura_config::available_memory_bytes(),
        )
        .expect("the embedded default is a ceiling on this machine");
        let warehouse = sutura_exec_datafusion::DataFusionWarehouse::new(
            (*source).clone(),
            posture,
            sutura_exec_datafusion::WorkingSet::of_bytes(ceiling.bytes()),
        )
        .expect("the engine starts");
        let data = fixture_root().join("data");
        for model in pinned.definitions().models().values() {
            let csv = data.join(format!("{}.csv", model.table()));
            warehouse
                .attach_csv(model.table_name(), &csv)
                .unwrap_or_else(|e| panic!("could not attach {}: {e}", csv.display()));
        }
        let engines = sutura_app::Warehouses::of(warehouse);

        let validated = sutura_app::verify_and_validate(pinned, &engines).expect("the converted bundle has no anchor to fail");
        let question_path = fixture_root().join("questions/total-revenue-on-the-fifteenth.yaml");
        let question: Query = serde_norway::from_str(
            &fs::read_to_string(&question_path).unwrap_or_else(|e| panic!("{} is readable: {e}", question_path.display())),
        )
        .unwrap_or_else(|e| panic!("{} is not a question: {e}", question_path.display()));

        let broker = sutura_config::StaticCredentialBroker::for_one_shared_source(
            SourceName::parse("wren").expect("the importer's own default source name is a name"),
            SharedIdentityDeclared::of(
                AcknowledgementReason::parse("a test fixture read by one process, as that process's own identity")
                    .expect("the fixture reason is a reason"),
            ),
        );
        let context = RequestContext::of(PrincipalChain::of(Subject::TheDeploymentItself));
        let deadline = Deadline::opened_at(
            time::Instant::now(),
            Budget::parse(time::Duration::from_secs(30)).expect("30s"),
        );
        let answered = sutura_app::answer(
            &validated,
            &question,
            &context,
            &broker,
            &engines,
            &sutura_exec_datafusion::DataFusionCombiner::new().expect("a combiner builds"),
            1 << 30,
            deadline,
            &sutura_app::SpendLedger::no_budget(),
            sutura_domain::plan::RowCeiling::DEFAULT,
        )
        .unwrap_or_else(|e| panic!("the converted catalog did not answer its own question: {e}"));

        match answered.into_outcome() {
            ToolOutcome::Refusal { reason } => panic!("the fixture's own question was refused: {reason:?}"),
            ToolOutcome::Answer { rows, .. } => {
                assert_eq!(
                    rows.columns(),
                    &["period".to_owned(), "sales_summary_total_revenue".to_owned()]
                );
                let one_row: &[Value] = rows.rows().first().expect("one day in range, one row");
                assert_eq!(rows.rows().len(), 1);
                assert_eq!(
                    one_row[1].render(),
                    "3500",
                    "the three 2026-01-15 orders, excluding the 16th's"
                );
            }
        }

        cleanup();
    }
}
