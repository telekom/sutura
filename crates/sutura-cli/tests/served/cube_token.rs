//! The Cube token, sent for real by the catalog reader and looked for everywhere the process answers
//! or writes (`telekom/sutura#1398`).
//!
//! The reader sends it when a `catalog.kind: cube` deployment boots, to a loopback Cube that accepts
//! it and to one that refuses it with a body naming it; each fake must have been sent the token as a
//! bearer, or the absence the scan reports proves nothing.
//!
//! **The limit:** the warehouse (`sutura-exec-cube`) sends the same token when a question is asked,
//! and nothing here reads that send. A deployment that asks Cube a question also verifies every
//! catalog anchor at boot against it, so a scripted fake would have to answer each anchor's queries
//! in an order the process chooses.

use sutura_catalog_cube::fixture::META;
use sutura_http_client::test_support::Scripted;

use crate::harness::{LOOPBACK, SINGLE_USER, TOKEN, VERSION, config_path, derived_beside};
use crate::no_echo::bearer_echoed;

const CANARY: &str = "5d27e8b1f90a4c63b7e0d9a2";

/// The recorded `meta` answer, which both readers fetch.
fn meta() -> Scripted {
    Scripted::ok(&serde_json::from_str(META).expect("the recorded meta answer is JSON"))
}

/// A directory holding the token file, removed on every path out.
struct TokenDir(std::path::PathBuf);

impl TokenDir {
    fn holding(case: &str) -> Self {
        let path = derived_beside(&config_path(case));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("the token directory is creatable");
        std::fs::write(path.join("token"), format!("{CANARY}\n")).expect("the token file is writable");
        Self(path)
    }

    fn file(&self) -> String {
        self.0.join("token").display().to_string()
    }
}

impl Drop for TokenDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn a_cube_catalog_token_is_held_out_of_every_reply_and_log_line_accepted_or_refused() {
    let case = "cube-catalog-no-echo";
    let token = TokenDir::holding(case);
    let leaked = bearer_echoed(case, CANARY, vec![meta(), meta()], |cube| {
        let (endpoint, file) = (cube.endpoint(), token.file());
        format!(
            "server:\n{LOOPBACK}security:\n{SINGLE_USER}  access_token: \"{TOKEN}\"\n\
             telemetry:\n  format: \"bunyan\"\n\
             catalogs:\n  - name: \"metrics\"\n    kind: \"cube\"\n    dir: \"/unused-for-cube\"\n    \
             data_dir: \"/unused-for-cube\"\n    version: \"{VERSION}\"\n    endpoint: \"{endpoint}\"\n    \
             token_file: \"{file}\"\n\
             sources:\n  metrics:\n    kind: \"cube\"\n    endpoint: \"{endpoint}\"\n    token_file: \"{file}\"\n    \
             posture: \"shared-service-user\"\n"
        )
    });
    assert!(leaked.is_empty(), "the cube token was sent back or logged: {leaked:?}");
}
