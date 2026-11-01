use std::path::PathBuf;

/// A scratch directory of this test's own, removed by the caller.
fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("sutura-cli-secret-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("a scratch directory is creatable");
    directory
}

/// A secret file past the bound is refused naming the key, never read whole into memory at boot.
#[test]
fn a_secret_file_larger_than_any_secret_is_refused_naming_the_key() {
    let directory = scratch("large");
    let file = directory.join("password");
    std::fs::File::create(&file)
        .and_then(|created| created.set_len(super::MAX_SECRET_BYTES as u64 + 1))
        .expect("the file is sized");
    let refusal = super::read_key("sources.warehouse.password_file", &file).map(drop);
    let _ignored = std::fs::remove_dir_all(&directory);
    let error = refusal.expect_err("a file larger than any secret is not a credential");
    assert!(error.contains("`sources.warehouse.password_file` at"), "{error}");
    assert!(error.contains("holds more than 65536 bytes"), "{error}");
}

/// A path that is not a regular file is refused before it is opened, so a FIFO cannot block boot.
#[test]
fn a_secret_path_that_is_not_a_regular_file_is_refused_before_it_is_opened() {
    let directory = scratch("directory");
    let refusal = super::read_key("sources.warehouse.password_file", &directory).map(drop);
    let _ignored = std::fs::remove_dir_all(&directory);
    let error = refusal.expect_err("a directory is not a credential");
    assert!(error.contains("is not a regular file"), "{error}");
}
