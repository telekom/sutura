//! Export the existing release attestations, without signing them again.
//!
//! Expectations come from the pre-attestation checksum snapshot and pushed image records, not
//! from the bundles. This checks JSON/DSSE/SLSA shape and exact unique subjects, NOT signatures,
//! certificates, transparency proofs or signer identity. Those remain the verifier's job.
//! Bash and base64 are ordinary sandbox tools; no new codec or dependency is needed here.
//! Inputs are trusted job outputs: reads and subprocess waits have no application budget, and
//! passing the payload as an argument is subject to the operating system's argv size limit.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::Verdict;

/// A subject is its full name AND SHA-256 digest: four image lists share one repository name.
type Subject = (String, String);

/// The unique subject set across all five bundles.
type Subjects = BTreeSet<Subject>;

/// Collect only after every input validates. A failed or repeated invocation cannot overwrite.
pub(crate) fn run(args: &[String]) -> Verdict {
    let [checksums, images, output, bundles @ ..] = args else {
        return Verdict::Usage;
    };
    if bundles.len() != 5 {
        eprintln!("collect-provenance: expected five bundle paths");
        return Verdict::Usage;
    }
    match collect(Path::new(checksums), Path::new(images), Path::new(output), bundles) {
        Ok(subjects) => {
            println!("collect-provenance: wrote five bundles covering {subjects} unique subjects");
            Verdict::Pass
        }
        Err(cause) => {
            eprintln!("collect-provenance: {cause}");
            Verdict::Fail
        }
    }
}

/// Write one JSON object per line, preserving the signed payload and all verification material.
fn collect(checksums: &Path, images: &Path, output: &Path, paths: &[String]) -> Result<usize, String> {
    let mut expected = asset_subjects(&read(checksums)?)?;
    for subject in image_subjects(&read(images)?)? {
        if !expected.insert(subject) {
            return Err(String::from("duplicate expected subject"));
        }
    }
    let mut actual = Subjects::new();
    let mut jsonl = String::new();
    for path in paths {
        let bundle: Value =
            serde_json::from_str(&read(Path::new(path))?).map_err(|error| format!("bundle must be one JSON record: {error}"))?;
        for subject in bundle_subjects(&bundle)? {
            if !actual.insert(subject) {
                return Err(String::from("duplicate bundled subject"));
            }
        }
        jsonl.push_str(&serde_json::to_string(&bundle).map_err(|error| format!("bundle JSON: {error}"))?);
        jsonl.push('\n');
    }
    if actual != expected {
        return Err(String::from("bundled subjects differ from release subjects"));
    }
    publish(output, jsonl.as_bytes())?;
    Ok(actual.len())
}

/// Complete the sibling before linking its final name; never replace an existing final or stage.
/// No fsync or kill cleanup: an interrupted stage remains, and the action refuses it next run.
fn publish(output: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("output requires a file name")?;
    let staging = output.with_file_name(format!(".{name}.tmp"));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)
        .map_err(|error| format!("cannot create provenance staging file: {error}"))?;
    let written = file.write_all(bytes).and_then(|()| file.flush());
    drop(file);
    let published = written
        .map_err(|error| format!("cannot write provenance staging file: {error}"))
        .and_then(|()| std::fs::hard_link(&staging, output).map_err(|error| format!("cannot publish provenance asset: {error}")));
    // Only reached after this invocation's create_new succeeded. Keep both errors if cleanup fails.
    let cleaned = std::fs::remove_file(&staging).map_err(|error| format!("cannot remove owned provenance staging file: {error}"));
    match (published, cleaned) {
        (Err(primary), Err(cleanup)) => Err(format!("{primary}; {cleanup}")),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), cleanup) => cleanup,
    }
}

/// A missing, unreadable or empty input is not an empty attestation set.
fn read(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if text.trim().is_empty() {
        return Err(format!("empty input: {}", path.display()));
    }
    Ok(text)
}

/// The SHA-256 spelling produced by the release snapshot and image push steps.
fn subject(name: &str, digest: &str) -> Result<Subject, String> {
    if name.is_empty() || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(String::from("subject requires a name and a 64-digit SHA-256"));
    }
    Ok((String::from(name), String::from(digest)))
}

/// Read sha256sum's text format; assets are basenames, and generated bundles are not subjects.
fn asset_subjects(text: &str) -> Result<Subjects, String> {
    let mut subjects = Subjects::new();
    for line in text.lines() {
        let (digest, name) = line.split_once("  ").ok_or("malformed checksum record")?;
        if name.contains('/') || name.ends_with(".sigstore.json") || name.ends_with(".intoto.jsonl") {
            return Err(String::from("checksum subject must be an original release asset"));
        }
        if subjects.iter().any(|(prior, _)| prior == name) {
            return Err(String::from("duplicate checksum subject"));
        }
        subjects.insert(subject(name, digest)?);
    }
    if subjects.is_empty() {
        return Err(String::from("no checksum subjects"));
    }
    Ok(subjects)
}

/// Match the existing action's tag removal, retaining a registry's optional port.
///
/// The records carry a `#` comment header describing the two kinds, and that header ships inside
/// the signed `image-digests.txt` asset - so a comment is part of the format, not a malformed
/// record. Every other reader of the file already anchors on `list`/`leaf` and skips it. Skipping
/// one hides no record: a commented-out list is a missing list, which the count below refuses.
fn image_subjects(text: &str) -> Result<Subjects, String> {
    let mut subjects = Subjects::new();
    let mut variants = BTreeSet::new();
    for line in text.lines() {
        if line.starts_with('#') {
            continue;
        }
        let words: Vec<_> = line.split_whitespace().collect();
        let [kind, variant, reference] = words.as_slice() else {
            return Err(String::from("malformed image record"));
        };
        if *kind == "leaf" {
            continue;
        }
        if *kind != "list" || !variants.insert(*variant) {
            return Err(String::from("unknown or duplicate image list"));
        }
        let (tagged, digest) = reference.rsplit_once("@sha256:").ok_or("image list requires SHA-256")?;
        let (name, tag) = tagged.rsplit_once(':').ok_or("image list requires a tag")?;
        if tag.is_empty() || tag.contains('/') || !subjects.insert(subject(name, digest)?) {
            return Err(String::from("invalid or duplicate image subject"));
        }
    }
    if subjects.len() != 4 {
        return Err(String::from("expected four unique image-list subjects"));
    }
    Ok(subjects)
}

/// Decode with the existing system tool, passing data as an argument rather than shell source.
fn decode(payload: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-o",
            "pipefail",
            "-c",
            "printf '%s' \"$1\" | base64 -d",
            "decode",
            payload,
        ])
        .env_remove("BASH_ENV")
        .output()
        .map_err(|error| format!("cannot decode bundle payload: {error}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Err(String::from("invalid or empty base64 in bundle"));
    }
    Ok(output.stdout)
}

/// Validate only the envelope/statement fields this exporter consumes, not cryptographic trust.
fn bundle_subjects(bundle: &Value) -> Result<Subjects, String> {
    if !matches!(
        bundle.get("mediaType").and_then(Value::as_str),
        Some("application/vnd.dev.sigstore.bundle.v0.3+json" | "application/vnd.dev.sigstore.bundle+json;version=0.3")
    ) || bundle
        .get("verificationMaterial")
        .and_then(Value::as_object)
        .is_none_or(serde_json::Map::is_empty)
    {
        return Err(String::from("missing Sigstore bundle envelope"));
    }
    let envelope = bundle.get("dsseEnvelope").ok_or("missing DSSE envelope")?;
    if envelope.get("payloadType").and_then(Value::as_str) != Some("application/vnd.in-toto+json") {
        return Err(String::from("unexpected DSSE payload type"));
    }
    let signatures = envelope
        .get("signatures")
        .and_then(Value::as_array)
        .ok_or("missing DSSE signatures")?;
    let [signature] = signatures.as_slice() else {
        return Err(String::from("expected one DSSE signature"));
    };
    drop(decode(
        signature.get("sig").and_then(Value::as_str).ok_or("missing DSSE signature")?,
    )?);
    let payload = envelope
        .get("payload")
        .and_then(Value::as_str)
        .ok_or("missing DSSE payload")?;
    let statement: Value =
        serde_json::from_slice(&decode(payload)?).map_err(|error| format!("malformed in-toto statement: {error}"))?;
    if statement.get("_type").and_then(Value::as_str) != Some("https://in-toto.io/Statement/v1")
        || statement.get("predicateType").and_then(Value::as_str) != Some("https://slsa.dev/provenance/v1")
        || !statement.get("predicate").is_some_and(Value::is_object)
    {
        return Err(String::from("expected an in-toto SLSA v1 statement"));
    }
    let entries = statement
        .get("subject")
        .and_then(Value::as_array)
        .ok_or("missing statement subjects")?;
    let mut subjects = Subjects::new();
    for entry in entries {
        let name = entry.get("name").and_then(Value::as_str).ok_or("missing subject name")?;
        let digest = entry
            .get("digest")
            .and_then(Value::as_object)
            .ok_or("missing subject digest")?;
        let hash = digest
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or("missing subject SHA-256")?;
        if digest.len() != 1 || !subjects.insert(subject(name, hash)?) {
            return Err(String::from("unexpected or duplicate bundled subject digest"));
        }
    }
    if subjects.is_empty() {
        return Err(String::from("no bundled subjects"));
    }
    Ok(subjects)
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::process::Command;

    use serde_json::{Value, json};

    use crate::Verdict;

    /// Synthetic material only: these fixtures make no cryptographic claim.
    struct Fixture {
        root: PathBuf,
        args: Vec<String>,
        bundles: Vec<Value>,
    }

    /// Keep encoding independent of the collector's decoder.
    fn encoded(statement: &Value) -> String {
        let text = serde_json::to_string(statement).expect("fixture JSON");
        let output = Command::new("bash")
            .args([
                "--noprofile",
                "--norc",
                "-o",
                "pipefail",
                "-c",
                "printf '%s' \"$1\" | base64 | tr -d '\\n'",
                "encode",
                &text,
            ])
            .env_remove("BASH_ENV")
            .output()
            .expect("base64 encodes the fixture");
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).expect("base64 is ASCII")
    }

    fn statement(subjects: &Value) -> Value {
        json!({
            "_type": "https://in-toto.io/Statement/v1",
            "predicateType": "https://slsa.dev/provenance/v1",
            "predicate": {}, "subject": subjects
        })
    }

    fn bundled(subjects: &Value) -> Value {
        json!({
            "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
            "verificationMaterial": { "certificate": { "rawBytes": "Zml4dHVyZQ==" } },
            "dsseEnvelope": {
                "payloadType": "application/vnd.in-toto+json",
                "payload": encoded(&statement(subjects)),
                "signatures": [{ "sig": "Zml4dHVyZQ==" }]
            }
        })
    }

    fn fixture(stem: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("sutura-provenance-{stem}-{}", std::process::id()));
        #[expect(clippy::create_dir, reason = "exclusive creation refuses stale fixture inputs")]
        std::fs::create_dir(&root).expect("new fixture directory");
        let hashes: Vec<_> = (0..6).map(|n| format!("{n:064x}")).collect();
        let assets = bundled(&json!([
            {"name": "runtime.tar.gz", "digest": {"sha256": hashes[0]}},
            {"name": "runtime.tar.gz.sha256", "digest": {"sha256": hashes[1]}}
        ]));
        let mut bundles = vec![assets];
        // The comment header verbatim from a released `image-digests.txt`, which the collector
        // read as three malformed records. Only the references below are synthetic.
        let mut images = String::from(
            "# kind name reference@digest\n\
             # list = multi-arch manifest list; pin this unless you want one architecture\n\
             # leaf = single-arch image, named by its binary key and rust target triple\n",
        );
        for (variant, digest) in ["glibc", "musl", "serve-glibc", "serve-musl"]
            .iter()
            .zip(hashes.iter().skip(2))
        {
            writeln!(
                images,
                "list {variant} registry.example.com:5000/test/runtime:v0-{variant}@sha256:{digest}"
            )
            .expect("write to a String");
            bundles.push(bundled(&json!([{
                "name": "registry.example.com:5000/test/runtime", "digest": {"sha256": digest}
            }])));
        }
        images.push_str("leaf native registry.example.com/test/runtime:v0@sha256:ignored\n");
        let checksums = format!("{}  runtime.tar.gz\n{}  runtime.tar.gz.sha256\n", hashes[0], hashes[1]);
        std::fs::write(root.join("subjects.sha256"), checksums).expect("checksum snapshot");
        std::fs::write(root.join("image-digests.txt"), images).expect("image records");
        let mut args: Vec<_> = ["subjects.sha256", "image-digests.txt", "sutura-provenance.intoto.jsonl"]
            .iter()
            .map(|name| root.join(name).to_str().expect("fixture path").to_owned())
            .collect();
        for (index, bundle) in bundles.iter().enumerate() {
            let path = root.join(format!("bundle-{index}.json"));
            std::fs::write(&path, serde_json::to_vec_pretty(bundle).expect("fixture JSON")).expect("bundle output");
            args.push(path.to_str().expect("fixture path").to_owned());
        }
        Fixture { root, args, bundles }
    }

    fn registered(args: &[String]) -> Verdict {
        let task = crate::TASKS
            .iter()
            .find(|task| task.name == "collect-provenance")
            .expect("registered collector");
        (task.run)(args)
    }

    #[test]
    fn the_registered_collector_preserves_all_five_records_and_refuses_overwrite() {
        let fixture = fixture("clean");
        let staging = fixture.root.join(".sutura-provenance.intoto.jsonl.tmp");
        let first = registered(&fixture.args);
        let bytes = std::fs::read(&fixture.args[2]);
        let clean_stage = staging.try_exists();
        let second = registered(&fixture.args);
        let retained = std::fs::read(&fixture.args[2]);
        let repeat_stage = staging.try_exists();
        // A distinct sentinel proves failed link promotion preserves an existing final's bytes.
        std::fs::write(&fixture.args[2], "preexisting final").expect("fixture final sentinel");
        let conflict = registered(&fixture.args);
        let sentinel = std::fs::read_to_string(&fixture.args[2]);
        let conflict_stage = staging.try_exists();
        // A stage we did not create is neither reused nor cleaned up.
        std::fs::write(&staging, "preexisting stage").expect("fixture stage sentinel");
        let stale = registered(&fixture.args);
        let stage_sentinel = std::fs::read_to_string(&staging);
        std::fs::remove_dir_all(&fixture.root).expect("remove only fixture");
        assert_eq!(first, Verdict::Pass);
        let bytes = bytes.expect("JSONL asset was written");
        let records: Vec<Value> = String::from_utf8(bytes.clone())
            .expect("JSONL UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("one bundle per line"))
            .collect();
        assert_eq!(
            records, fixture.bundles,
            "payload, signature and verification material must survive export"
        );
        assert_eq!(second, Verdict::Fail);
        assert_eq!(
            retained.expect("original output retained"),
            bytes,
            "a repeated run must not replace an existing asset"
        );
        assert_eq!(conflict, Verdict::Fail);
        assert_eq!(sentinel.expect("final sentinel retained"), "preexisting final");
        assert_eq!(
            [
                clean_stage.expect("stage readable"),
                repeat_stage.expect("stage readable"),
                conflict_stage.expect("stage readable")
            ],
            [false, false, false],
            "success and failed promotion must remove owned staging"
        );
        assert_eq!(stale, Verdict::Fail);
        assert_eq!(stage_sentinel.expect("unowned stage retained"), "preexisting stage");
    }

    #[test]
    fn invalid_bundle_outputs_never_create_a_release_asset() {
        let fixture = fixture("invalid");
        let clean = fixture.bundles[4].clone();
        let mut wrong_type = clean.clone();
        wrong_type["dsseEnvelope"]["payloadType"] = json!("text/plain");
        let mut bad_payload = clean.clone();
        bad_payload["dsseEnvelope"]["payload"] = json!("!");
        let mut bad_statement = clean.clone();
        bad_statement["dsseEnvelope"]["payload"] = encoded(&json!({"subject": []})).into();
        let mut no_signature = clean;
        no_signature["dsseEnvelope"]["signatures"] = json!([]);
        let mut cases = vec![
            ("empty", String::new()),
            ("malformed", String::from("{")),
            ("two records", String::from("{}\n{}\n")),
            ("no envelope", String::from("{}")),
        ];
        for (name, bundle) in [
            ("wrong payload type", wrong_type),
            ("invalid base64", bad_payload),
            ("wrong statement", bad_statement),
            ("no signature", no_signature),
            ("duplicate subject", fixture.bundles[3].clone()),
            (
                "extra subject",
                bundled(&json!([{"name": "unexpected", "digest": {"sha256": "a".repeat(64)}}])),
            ),
            (
                "wrong digest",
                bundled(&json!([{"name": "registry.example.com:5000/test/runtime", "digest": {"sha256": "f".repeat(64)}}])),
            ),
            ("no subjects", bundled(&json!([]))),
        ] {
            cases.push((name, serde_json::to_string(&bundle).expect("fixture JSON")));
        }
        let mut observed = Vec::new();
        for (name, bytes) in cases {
            std::fs::write(&fixture.args[7], bytes).expect("replace only fixture bundle");
            observed.push((name, registered(&fixture.args), PathBuf::from(&fixture.args[2]).exists()));
        }
        std::fs::remove_file(&fixture.args[7]).expect("remove only fixture bundle");
        observed.push((
            "missing path",
            registered(&fixture.args),
            PathBuf::from(&fixture.args[2]).exists(),
        ));
        std::fs::remove_dir_all(&fixture.root).expect("remove only fixture");
        for (case, verdict, exists) in observed {
            assert_eq!((verdict, exists), (Verdict::Fail, false), "{case}");
        }
    }

    #[test]
    fn expected_subjects_are_read_from_release_inputs_not_inferred_from_bundles() {
        let fixture = fixture("expected");
        let checksums = std::fs::read_to_string(&fixture.args[0]).expect("original checksums");
        let images = std::fs::read_to_string(&fixture.args[1]).expect("original image records");
        let mut observed = Vec::new();
        for (index, name, bytes) in [
            (0, "empty checksums", String::new()),
            (
                0,
                "missing asset",
                checksums.lines().next().expect("first checksum").to_owned(),
            ),
            (0, "duplicate asset", format!("{checksums}{checksums}")),
            (
                0,
                "recursive export",
                checksums.replace("runtime.tar.gz.sha256", "sutura-provenance.intoto.jsonl"),
            ),
            (
                1,
                "missing list",
                images
                    .lines()
                    .filter(|line| !line.starts_with("list glibc "))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            (
                1,
                "changed list digest",
                images.replace(&format!("{:064x}", 5), &"e".repeat(64)),
            ),
            // A record short of a field is still refused, and a `#` skip is not a way to drop a
            // list: commenting one out is a missing list, not an accepted one.
            (1, "malformed record", images.replace("list glibc ", "list ")),
            (1, "commented-out list", images.replace("list glibc ", "# list glibc ")),
        ] {
            std::fs::write(&fixture.args[index], bytes).expect("replace one expected input");
            observed.push((name, registered(&fixture.args), PathBuf::from(&fixture.args[2]).exists()));
            std::fs::write(&fixture.args[0], &checksums).expect("restore checksums");
            std::fs::write(&fixture.args[1], &images).expect("restore image records");
        }
        let clean = registered(&fixture.args);
        let count = std::fs::read_to_string(&fixture.args[2])
            .expect("restored export")
            .lines()
            .count();
        std::fs::remove_dir_all(&fixture.root).expect("remove only fixture");
        for (case, verdict, exists) in observed {
            assert_eq!((verdict, exists), (Verdict::Fail, false), "{case}");
        }
        assert_eq!((clean, count), (Verdict::Pass, 5));
    }
}
