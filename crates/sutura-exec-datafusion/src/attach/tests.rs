//! The codec parse, and a compressed file read end to end.
//!
//! **Two codecs have an end-to-end cell and two do not, and the split is deliberate.** `gz` and
//! `bz2` are written here by a dev-dependency writer and read back through the engine, because
//! those two are what the supply-chain decision in `docs/adr/0039` actually bought: `bz2` is the
//! licence entry `deny.toml` gained, so a cell that never reads one would leave that entry
//! justified by nothing. `xz` and `zst` are parse-only - no writer is declared for them - so what
//! is proven for those two is that the suffix maps to the right codec, NOT that this build decodes
//! them. That limit is the reason the sentence is here rather than in a commit message.

use std::path::Path;

use sutura_domain::plan::Executable;
use sutura_domain::warehouse::{Value, Warehouse as _};

use super::Codec;
use crate::{DataFusionError, DataFusionWarehouse};

/// Where this module's fixtures are written. One directory per cell, removed on the way in and out.
fn scratch(name: &str) -> std::path::PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp").join(name);
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("the fixture directory is writable");
    dir
}

/// Two rows of one table, in the shape `crate::execute_tests`'s plan reads.
const CSV: &str = "order_date,region,amount_cents\n2026-06-05,north,3\n2026-06-20,north,4\n";

fn gzipped(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("a vector accepts bytes");
    encoder.finish().expect("the gzip member closes")
}

fn bzipped(bytes: &[u8]) -> Vec<u8> {
    use std::io::Write as _;
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    encoder.write_all(bytes).expect("a vector accepts bytes");
    encoder.finish().expect("the bzip2 stream closes")
}

fn engine() -> DataFusionWarehouse {
    DataFusionWarehouse::new(
        crate::source(),
        crate::test_posture(),
        crate::WorkingSet::of_bytes(core::num::NonZeroUsize::new(64 * 1024 * 1024).expect("a test ceiling is positive")),
    )
    .expect("a current-thread runtime builds")
}

/// Answers the one question `crate::execute_tests` builds, against whatever is attached as `orders`.
fn total(adapter: &DataFusionWarehouse) -> Vec<Vec<Value>> {
    let query = crate::question();
    adapter
        .execute(Executable::Query(&query), &crate::test_leg(), crate::test_deadline())
        .map(|batches| crate::decoded(&batches))
        .expect("the plan runs")
        .rows()
        .to_vec()
}

/// The whole point of the feature: a gzipped CSV answers the same number the plain one does.
#[test]
fn a_gzipped_csv_answers_the_same_total_as_the_plain_one() {
    let dir = scratch("datafusion-attach-gzip");
    let plain = dir.join("plain.csv");
    let compressed = dir.join("orders.csv.gz");
    std::fs::write(&plain, CSV).expect("the plain fixture is writable");
    std::fs::write(&compressed, gzipped(CSV.as_bytes())).expect("the gzipped fixture is writable");

    let uncompressed = engine();
    uncompressed
        .attach_csv(&crate::orders(), &plain)
        .expect("a plain CSV attaches");
    let zipped = engine();
    zipped
        .attach_csv(&crate::orders(), &compressed)
        .expect("a gzipped CSV attaches");

    assert_eq!(total(&zipped), total(&uncompressed));
    assert_eq!(
        total(&zipped),
        [vec![
            Value::Text(String::from("north")),
            Value::Text(String::from("2026-06-01")),
            Value::Integer(7),
        ]]
    );
    drop(std::fs::remove_dir_all(dir));
}

/// The codec the licence entry was added for, read rather than merely allowed.
#[test]
fn a_bzipped_csv_reads_through_the_codec_the_licence_entry_names() {
    let dir = scratch("datafusion-attach-bzip2");
    let compressed = dir.join("orders.csv.bz2");
    std::fs::write(&compressed, bzipped(CSV.as_bytes())).expect("the bzipped fixture is writable");

    let adapter = engine();
    adapter
        .attach_csv(&crate::orders(), &compressed)
        .expect("a bzipped CSV attaches");
    assert_eq!(
        total(&adapter),
        [vec![
            Value::Text(String::from("north")),
            Value::Text(String::from("2026-06-01")),
            Value::Integer(7),
        ]]
    );
    drop(std::fs::remove_dir_all(dir));
}

/// The other plain-text format, which had no affordance at all before this change.
#[test]
fn a_gzipped_ndjson_file_answers_the_same_total_as_a_csv() {
    let dir = scratch("datafusion-attach-json");
    let compressed = dir.join("orders.ndjson.gz");
    let records = "{\"order_date\":\"2026-06-05\",\"region\":\"north\",\"amount_cents\":3}\n\
                   {\"order_date\":\"2026-06-20\",\"region\":\"north\",\"amount_cents\":4}\n";
    std::fs::write(&compressed, gzipped(records.as_bytes())).expect("the gzipped fixture is writable");

    let adapter = engine();
    adapter
        .attach_json(&crate::orders(), &compressed)
        .expect("a gzipped NDJSON file attaches");
    assert_eq!(
        total(&adapter),
        [vec![
            Value::Text(String::from("north")),
            Value::Text(String::from("2026-06-01")),
            Value::Integer(7),
        ]]
    );
    drop(std::fs::remove_dir_all(dir));
}

/// Every suffix this build reads, mapped to the codec it names.
#[test]
fn every_declared_suffix_parses_to_its_own_codec() {
    let expected = [
        ("orders.csv", Codec::None),
        ("orders.csv.gz", Codec::Gzip),
        ("orders.csv.bz2", Codec::Bzip2),
        ("orders.csv.xz", Codec::Xz),
        ("orders.csv.zst", Codec::Zstd),
        // Case-insensitive: a file written on a system that upper-cased the extension still reads.
        ("orders.csv.GZ", Codec::Gzip),
        // Not a codec, and not a near miss either: a CSV may legitimately be called this.
        ("orders.txt", Codec::None),
        ("orders", Codec::None),
    ];
    for (name, codec) in expected {
        assert_eq!(
            Codec::of_path(Path::new(name)).as_ref().ok(),
            Some(&codec),
            "{name} should parse as {codec:?}"
        );
    }
}

/// A codec spelled a way this build does not read is refused rather than read as text.
///
/// The failure being prevented is silent: the engine would infer a schema from the codec's header
/// bytes and resolve the table to columns nobody declared.
#[test]
fn a_misspelled_codec_is_refused_by_name_rather_than_read_as_text() {
    let refused = Codec::of_path(Path::new("orders.csv.gzip"));
    assert!(
        matches!(refused, Err(DataFusionError::UnknownCodec { ref suffix, .. }) if suffix == "gzip"),
        "a `.gzip` suffix should be refused naming itself, got {refused:?}"
    );
    for name in ["orders.csv.zip", "orders.csv.lz4", "orders.csv.br", "orders.csv.7z"] {
        assert!(
            matches!(Codec::of_path(Path::new(name)), Err(DataFusionError::UnknownCodec { .. })),
            "{name} should be refused"
        );
    }
}

/// An outer codec around Parquet is refused, because Parquet's own codecs are inside the file.
#[test]
fn an_outer_codec_around_parquet_is_refused_rather_than_unwrapped() {
    let dir = scratch("datafusion-attach-parquet-gz");
    let path = dir.join("orders.parquet.gz");
    std::fs::write(&path, gzipped(b"not really parquet")).expect("the fixture is writable");
    let refused = engine().attach_parquet(&crate::orders(), &path);
    assert!(
        matches!(refused, Err(DataFusionError::UnknownCodec { .. })),
        "a gzipped Parquet file should be refused, got {refused:?}"
    );
    drop(std::fs::remove_dir_all(dir));
}

/// The suffixes a composition root offers are the suffixes the parse accepts.
///
/// One table, read two ways: a `sutura_cli` file search that spelled its own list could offer a
/// candidate `attach_csv` then refuses, or miss one it reads.
#[test]
fn the_offered_suffixes_are_exactly_the_parsed_ones() {
    let offered: Vec<&str> = Codec::suffixes().collect();
    assert_eq!(offered, ["gz", "bz2", "xz", "zst"]);
    for suffix in offered {
        let path = format!("orders.csv.{suffix}");
        assert_ne!(
            Codec::of_path(Path::new(path.as_str())).ok(),
            Some(Codec::None),
            "{path} is offered, so it must parse as a codec"
        );
    }
}
