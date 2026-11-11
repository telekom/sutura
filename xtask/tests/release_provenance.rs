#![forbid(unsafe_code)]
//! Exercise the registered collector with synthetic bundles, without signing or publication.

#![cfg(test)]

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

fn bundle(subject: &Value) -> Value {
    let statement = json!({
        "_type": "https://in-toto.io/Statement/v1",
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {}, "subject": [subject]
    });
    let encoded = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-o",
            "pipefail",
            "-c",
            "printf '%s' \"$1\" | base64 | tr -d '\\n'",
            "encode",
            &serde_json::to_string(&statement).expect("fixture statement"),
        ])
        .env_remove("BASH_ENV")
        .output()
        .expect("encode fixture payload");
    assert!(encoded.status.success());
    json!({
        "mediaType": "application/vnd.dev.sigstore.bundle.v0.3+json",
        "verificationMaterial": {"certificate": {"rawBytes": "Zml4dHVyZQ=="}},
        "dsseEnvelope": {
            "payloadType": "application/vnd.in-toto+json",
            "payload": String::from_utf8(encoded.stdout).expect("base64 is ASCII"),
            "signatures": [{"sig": "Zml4dHVyZQ=="}]
        }
    })
}

#[test]
fn release_provenance_requires_both_sql_lists_and_all_six_exact_names() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("release-six-lists-{}", std::process::id()));
    if root.exists() {
        std::fs::remove_dir_all(&root).expect("sweep stale fixture");
    }
    std::fs::create_dir_all(&root).expect("fixture directory");
    let checksums = root.join("subjects.sha256");
    let images = root.join("image-digests.txt");
    let output = root.join("export.jsonl");
    std::fs::write(&checksums, format!("{:064x}  runtime.tar.gz\n", 0)).expect("checksum snapshot");
    let mut records = String::new();
    let mut paths = Vec::new();
    for (index, variant) in [
        "assets",
        "glibc",
        "musl",
        "operator-glibc",
        "operator-musl",
        "sql-glibc",
        "sql-musl",
    ]
    .iter()
    .enumerate()
    {
        let name = if index == 0 {
            "runtime.tar.gz"
        } else {
            "registry.example.com/test/runtime"
        };
        if index != 0 {
            writeln!(records, "list {variant} {name}@sha256:{index:064x}").expect("image record");
        }
        let path = root.join(format!("{variant}.json"));
        let subject = json!({"name": name, "digest": {"sha256": format!("{index:064x}")}});
        std::fs::write(&path, serde_json::to_vec(&bundle(&subject)).expect("fixture bundle")).expect("bundle file");
        paths.push(path);
    }
    for (case, records, accepted) in [
        ("complete", records.clone(), true),
        (
            "missing SQL glibc",
            records
                .lines()
                .filter(|line| !line.starts_with("list sql-glibc "))
                .collect::<Vec<_>>()
                .join("\n"),
            false,
        ),
        (
            "missing SQL musl",
            records
                .lines()
                .filter(|line| !line.starts_with("list sql-musl "))
                .collect::<Vec<_>>()
                .join("\n"),
            false,
        ),
        (
            "unknown sixth name",
            records.replace("list sql-musl ", "list unexpected "),
            false,
        ),
        ("duplicate name", records.replace("list sql-musl ", "list sql-glibc "), false),
    ] {
        std::fs::write(&images, records).expect("image records");
        let result = Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("collect-provenance")
            .args([&checksums, &images, &output])
            .args(&paths)
            .output()
            .expect("execute real collector");
        assert_eq!(result.status.code(), Some(i32::from(!accepted)), "{case}: {result:?}");
        assert_eq!(output.exists(), accepted, "{case}");
        if accepted {
            let exported = std::fs::read_to_string(&output)
                .expect("export")
                .lines()
                .inspect(|line| {
                    drop(serde_json::from_str::<Value>(line).expect("exported bundle"));
                })
                .count();
            assert_eq!(exported, 7);
            std::fs::remove_file(&output).expect("remove accepted export before refusal cases");
        }
    }
    std::fs::remove_dir_all(root).expect("sweep fixture");
}
