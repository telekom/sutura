//! Wave one of the identity-aware E2E (`just e2e-datahub-bigquery`): `DataHub` carries the certified
//! metric's definition, a REAL Keycloak issuer's token says who is asking, and a multi-source
//! deployment answers it - over HTTP `/v1/query`, on the composed `sutura-serve` binary, with the
//! same recorded corpus `served/datahub.rs` certifies against.
//!
//! # The wave's claim, and how this file is split to hold it honestly
//!
//! Issue #134's whole path is **data metadata → a real issuer's token → a source that executes AS
//! the asking subject**. This file holds BOTH halves, but deliberately as TWO cells, because only
//! one of them is runnable before the maintainer's binding (issue #376 P2) lands.
//!
//! - **`the_wave_one_path_answers_as_the_asking_subject`** - the runnable wave. It boots a
//!   deployment once and asks it as principal A, with an uncertified request, and as principal B -
//!   same binary, same settings file, same catalog, three asks. It proves the three claims PR 1
//!   can: (1) a `catalog.kind: datahub` deployment serves the certified metric over a real
//!   issuer's verified token; (2) an uncertified question is a typed refusal, never `200`; (3) two
//!   different provisioned subjects produce two different audit `subject`s. `DataHub` is the
//!   recorded fake (`#202`'s `test_support`), the source is `files` named identically to the
//!   catalog (the served `datahub` arm's fixed `bigquery`→name mapping, exactly as
//!   `served/datahub.rs` proves green), and the issuer is the provisioned Keycloak tier.
//! - **`the_source_executes_as_the_asking_subject`** - the exchanged-identity half, **NOT invoked
//!   by the task**. Reading `SESSION_USER()` as the subject, per subject, is exactly the row
//!   `docs/where-identity-is-proven.md` keeps **`unrun`**: it needs the `iamcredentials` hop to a
//!   service account (#376 P2) and per-subject assertions in `bq-test`, neither of which is shipped.
//!   It stays `#[ignore]`d and says so rather than being written as if it could run.
//!
//! # What is ONE cell rather than three
//!
//! The runnable wave is one `#[ignore]`d cell that boots the deployment once and asks three things.
//! Each `#[test]` would re-boot the deployment, and a boot RE-EXECUTES every anchor against the
//! engine (`served/harness.rs`'s `START_BUDGET` doc says so) plus reads the catalog's pages from
//! the fake another time - no additional evidence for the money. The two-subject property
//! especially needs one boot: the point is that one deployment answers two different callers as two
//! different subjects, which three boots would not show.
//!
//! # The dependency split, stated next to the claim
//!
//! This file compiles only inside `served.rs`'s `#[cfg(feature = "datahub")]` +
//! `#[cfg(feature = "bigquery")]` module declaration (the `datahub` gate is the reader crate, the
//! `bigquery` gate that this is the wave whose source is the per-subject-capable adapter), and is
//! `#[ignore]`d so none of it runs on `just test`. Each dependency marks its requirement:
//!
//! - **`// requires #202`** (PR1 `#720` landed, PR2 `#750` is the base) - `catalog.kind: datahub`
//!   served, the settings keys `endpoint`/`token_file`/`metric_property`, and
//!   `sutura_catalog_datahub::test_support::{FakeServer, happy_path_answers, DEPLOYMENT_PROPERTY}`.
//! - **`// requires WKC`** - `harness::keycloak_settings` (the `KeycloakFixture`) and the realm the
//!   tier writes at `start`.
//! - **`// requires #376 P2`** - ONLY the exchanged-identity cell below, which is not run by the task.
//!
//! # RED/GREEN
//!
//! **The cells are `#[ignore]`d, so `just test` does not reach them** - the task
//! (`just e2e-datahub-bigquery`, or the nix app `apps.e2e-datahub-bigquery`) brings the Keycloak
//! tier up and runs the runnable cell by name. Mutations this runnable cell guards: the served
//! `datahub` arm rolled back to its unconditional refusal (RED - the deployment never boots, the
//! `refused_to_start` shape); the Keycloak fixture or the realm reader removed (RED - does not
//! compile / the provider is gone); the audit `subject` collapsing to the deployment's own (RED -
//! two provisioned subjects no longer differ). GREEN is this file as written.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use sutura_catalog_datahub::test_support::{DEPLOYMENT_PROPERTY, FakeServer, happy_path_answers};

    // requires WKC
    use crate::harness::keycloak::KeycloakFixture;
    use crate::harness::{
        LOOPBACK, RECORD, VERSION, config_path, derived_beside, keycloak_settings, keycloak_subject_of, start_configured, v1,
    };

    /// The `case` string this file's deployment owns - handed to [`keycloak_settings`] (which writes
    /// its fetched key set beside it) and to [`start_configured`] (which writes the settings file and
    /// clears only the config dir, a sibling of that key set - `served/harness.rs`'s
    /// `derived_beside`). The two must agree, because the `inbound` block this file writes names the
    /// key set at exactly the path the fixture wrote.
    const CASE: &str = "e2e-datahub-bigquery";

    /// The catalog's declared name, reused verbatim as the `sources:` entry's name - the served
    /// `datahub` arm fixes the `bigquery` dataPlatform→source mapping to the CATALOG's OWN name
    /// (`crates/sutura-serve/src/catalog.rs`), so a source under any other name is unreachable by
    /// the harvested models. The same identity `served/datahub.rs` uses.
    const CATALOG: &str = "metrics";

    /// The data directory this case owns and removes on every path out, the same promise
    /// `served/datahub.rs`'s `DataDir` makes - it holds the CSV the `files` engine reads (named
    /// after the model's `orders` table) plus the catalog's token file, a sibling of the settings
    /// directory so `written()` (via `start_configured`) cannot wipe it.
    struct DataDir(PathBuf);

    impl DataDir {
        fn prepared(case: &str) -> Self {
            let path = derived_beside(&config_path(case));
            drop(std::fs::remove_dir_all(&path));
            std::fs::create_dir_all(&path).expect("the data directory is creatable");
            // `orders`'s rows, byte for byte `served/datahub.rs`'s `ORDERS_CSV`: three active orders
            // inside the certified anchor's June window summing to its declared value, one cancelled
            // order (excluded by the metric's status filter) and one active May order (excluded by
            // the question's range) - so a filter or a range mistake moves the total rather than
            // leaving it right by accident.
            std::fs::write(
                path.join("orders.csv"),
                "order_id,customer_id,amount_cents,order_date,status\n\
                 O1,C1,200000,2026-06-01,active\n\
                 O2,C2,150000,2026-06-01,active\n\
                 O3,C1,62345,2026-06-01,active\n\
                 O4,C2,999999,2026-06-01,cancelled\n\
                 O5,C1,111111,2026-05-01,active\n",
            )
            .expect("orders.csv is writable");
            std::fs::write(path.join("customers.csv"), "customer_id,segment\nC1,retail\nC2,wholesale\n")
                .expect("customers.csv is writable");
            std::fs::write(path.join("token"), "pat-under-test").expect("the token file is writable");
            Self(path)
        }

        fn token_file(&self) -> PathBuf {
            self.0.join("token")
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// The settings a wave-one deployment needs: the `datahub` catalog pointed at the fake
    /// (`#202`'s recorded corpus, served), the `files` source under the catalog's own name (`kind:
    /// files` - nothing here needs a real `BigQuery`, for the same reason `served/datahub.rs`
    /// proves green: the fixed `bigquery`→name mapping only cares that the NAME matches), and the
    /// Keycloak issuer's own `inbound` (`mode: direct`, so the caller's token IS the identity - no
    /// `security.access_token`, because a deployment declaring both is refused as
    /// `DeploymentTokenSharesTheHeader`).
    ///
    /// `dir`/`data_dir` on the catalog are the two path fields `CatalogSettings` requires non-empty
    /// for EVERY kind including `datahub`, unread by the datahub opener - the same obviously-unused
    /// placeholders `served/datahub.rs` declares.
    fn settings(fixture: &KeycloakFixture, server: &FakeServer, data: &DataDir) -> String {
        let key_set = derived_beside(&config_path(CASE)).join("keycloak-jwks.json");
        format!(
            "server:\n\
             {LOOPBACK}\
             security:\n\
             {SECURITY_HEAD}\
             {inbound}\
             telemetry:\n  \
               format: \"bunyan\"\n\
             catalogs:\n  \
               - name: \"{CATALOG}\"\n    \
                 kind: \"datahub\"\n    \
                 dir: \"/unused-for-datahub\"\n    \
                 data_dir: \"/unused-for-datahub\"\n    \
                 version: \"{VERSION}\"\n    \
                 endpoint: \"{endpoint}\"\n    \
                 token_file: \"{token_file}\"\n    \
                 metric_property: \"{DEPLOYMENT_PROPERTY}\"\n\
             sources:\n  \
               {CATALOG}:\n    \
                 kind: \"files\"\n    \
                 data_dir: \"{data_dir}\"\n    \
                 posture: \"shared-service-user\"\n",
            inbound = inbound_block(fixture, &key_set),
            endpoint = server.endpoint(),
            token_file = data.token_file().display(),
            data_dir = data.0.display(),
        )
    }

    /// The `security.inbound` block. The audience is the fixture's own `resource` (read off the
    /// minted token), the issuer comes from the realm document the fixture fetched, and
    /// `algorithms` pins `RS256` because that is what the tier's generated keys verify with -
    /// every fact from the fixture, never copied. `token_type: "any"` for the same measured reason
    /// `harness/keycloak.rs`'s `settings_naming` carries it: the tier mints `typ: JWT`, not the
    /// `at+jwt` the default would refuse.
    fn inbound_block(fixture: &KeycloakFixture, key_set: &Path) -> String {
        format!(
            "  inbound:\n    mode: \"direct\"\n    resource: \"{resource}\"\n    \
             authorization_server: \"{issuer}\"\n    key_set_file: \"{key_set}\"\n    \
             algorithms: [\"RS256\"]\n    token_type: \"any\"\n",
            resource = fixture.resource,
            issuer = fixture.issuer,
            key_set = key_set.display(),
        )
    }

    /// The head of the `security:` block this deployment makes: `single-user` identity - the same
    /// head every other proving-green served cell in this suite uses (`served/datahub.rs`,
    /// `harness/keycloak.rs`'s `settings_naming`) - so the source executes as this process, and who
    /// is asking is still established per request through `inbound` and recorded in the audit
    /// `subject`. **The per-subject EXECUTION is not claimed here**: that is the exchanged-identity
    /// cell below, which stays `#[ignore]`d behind #376 P2, and `docs/where-identity-is-proven.md`
    /// keeps `unrun`. `single-user` refuses a missing `single_user_because`, so one is written.
    const SECURITY_HEAD: &str = "  identity: \"single-user\"\n  single_user_because: \"wave one's served \
                                fixture reads its own recorded corpus as one identity, whoever asks - the \
                                per-subject exchange is #376 P2\"\n";

    /// The one question this path asks as both principals, over exactly the certified metric's
    /// anchor range - the same shape `served/datahub.rs`'s `QUESTION` holds, so the answer this
    /// file pins is the same value that file already proves.
    const QUESTION: &str = r#"{"metric":"revenue","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// A question this catalog does NOT certify - the wave's refusal leg, presented with a VALID
    /// principal token so what is refused is the question, not the caller's signature. The metric
    /// name is deliberately not one the recorded corpus defines.
    const UNCERTIFIED: &str =
        r#"{"metric":"definitely_not_certified","grain":"month","range":{"start":"2026-06-01","end":"2026-07-01"}}"#;

    /// The `subject` field of one bunyan record line - the same read `served.rs`'s keycloak cell
    /// makes: two real Keycloak subjects mint two different `sub` claims, and this is how the audit
    /// assertion tells whether the record carries the ASKER'S subject.
    fn subject_field(line: &str) -> &str {
        let after = line
            .split_once(r#""subject":""#)
            .map_or_else(|| panic!("no `subject` field in the record: {line}"), |(_, rest)| rest);
        after
            .split_once('"')
            .map_or_else(|| panic!("the `subject` field is not terminated: {line}"), |(value, _)| value)
    }

    #[test]
    #[ignore = "needs the Keycloak tier and the served datahub; run via `just e2e-datahub-bigquery`, which brings the tier up first"]
    fn the_wave_one_path_answers_as_the_asking_subject() {
        // One boot, three asks. The Keycloak fixture mints both subjects and writes the key set the
        // `inbound` block names; the datahub fake serves the recorded corpus twice - once for the
        // engine-open load and once inside `LocalService::start_composed`, which loads the catalog it
        // serves rather than trusting the bundle it was handed (`crates/sutura-serve/src/main.rs`
        // says so), the same six connections `served/datahub.rs`'s cell accounts for.
        // The keycloak fixture writes its fetched key set into `derived_beside(&config_path(CASE))`
        // - the SAME directory this file's `DataDir` owns - so the data directory is prepared FIRST
        // (removing any stale one) and the fixture's `create_dir_all`/write that follows is what
        // leaves the key set intact for the deployment the `inbound` block names. The reverse order
        // would wipe the just-fetched key set and the deployment would refuse a missing file.
        let data = DataDir::prepared(CASE);
        let fixture = keycloak_settings(CASE);
        let mut answers = happy_path_answers();
        answers.extend(happy_path_answers());
        let server = FakeServer::start(answers);
        let deployment = start_configured(CASE, &settings(&fixture, &server, &data));

        // Ask 1: principal A's Keycloak-minted token asks the certified DataHub-harvested metric.
        let reply = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_a_token),
            QUESTION,
        );
        assert_eq!(reply.status, 200, "{}", reply.body);
        let body = reply.json();
        assert_eq!(body["outcome"], "answer", "{}", reply.body);
        assert_eq!(body["rows"], serde_json::json!([["2026-06-01", "412345"]]), "{}", reply.body);
        assert_eq!(
            body["executed_as"],
            serde_json::json!([{ "source": CATALOG, "posture": "shared-service-user" }]),
            "{}",
            reply.body
        );
        // The token file actually reached the wire - the same assertion `served/datahub.rs` makes:
        // a composition root that ignored the declared token file would still answer the question.
        // `finish` joins the fake thread; the audit record is written by the deployment's own
        // blocking pool, so this reads it after the fake is reaped with a bounded sweep of the
        // deployment's log rather than the fixed `awaiting` deadline.
        let authorizations = server.finish();
        assert!(
            !authorizations.is_empty(),
            "the served binary sent no DataHub page request at all"
        );
        assert!(
            authorizations
                .iter()
                .all(|seen| seen.as_deref() == Some("Bearer pat-under-test")),
            "every DataHub page request must carry the token_file's bearer, got: {authorizations:?}"
        );
        // The audit record arrives with the answer, read straight from the deployment's log - the
        // sweep and the `awaiting` deadline both fight the channel's behavior after the fake is
        // reaped, so this reads once, after `finish` has joined the fake's thread. (`log()` drains
        // the channel, so this read is what asks 2a/2b/3's later reads must not repeat.)
        let lines_a = deployment.log();
        assert!(
            lines_a
                .iter()
                .rev()
                .any(|line| line.contains(r#""subject_established":"verified""#)),
            "ask A's record does not say a caller was verified:\n{lines_a:?}"
        );
        // The audit record carries the MASKED subject (`mask_principal_into` in `sutura-domain`'s
        // `principal` masks a `sub` to its first character plus `***`), so two UUIDs sharing a
        // first hex character collide 1 in 16 - a bare `assert_ne!` on the records would pass on
        // that prefix alone. The property rests instead on the full `sub` each token's OWN payload
        // mints (the harness decoded it), and each record is tied to ITS OWN token's mask - the
        // same pattern `served/keycloak_test.rs` proves green. Uniqueness is `sub_a != sub_b`;
        // attribution to the record is the `assert_eq!` through the same one-hex mask.
        let sub_a = keycloak_subject_of(&fixture.subject_a_token);
        let sub_b = keycloak_subject_of(&fixture.subject_b_token);
        assert_ne!(sub_a, sub_b, "two provisioned subjects minted the same `sub` claim");
        let mask = |sub: &str| {
            sutura_domain::identity::SubjectId::parse(sub)
                .expect("a Keycloak UUID parses as a subject")
                .to_string()
        };
        let subject_a = lines_a
            .iter()
            .rev()
            .find(|line| line.contains(RECORD))
            .map(|line| subject_field(line))
            .expect("ask A produced an audit record carrying its subject");
        assert_eq!(
            subject_a,
            mask(&sub_a),
            "A's record does not carry the mask of A's own token's subject"
        );

        // Ask 2a: NO credential - the bearer gate refuses before the question is ever looked at.
        let unauth = deployment.post(&v1(sutura_http::constants::base_paths::QUERY), None, QUESTION);
        assert_eq!(
            unauth.status, 401,
            "a request with no credential was answered: {}",
            unauth.body
        );
        assert_eq!(unauth.json()["code"], "unauthorized", "{}", unauth.body);

        // Ask 2b: a VALID principal token, but a question this catalog does not certify - a typed
        // refusal with its reason, never `200`. What is refused is the question, not the caller.
        let refused = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_a_token),
            UNCERTIFIED,
        );
        assert_ne!(refused.status, 200, "an uncertified question was answered: {}", refused.body);
        let refused_body = refused.json();
        // A typed refusal: the `reason` object carries the stable `code` and the `detail` sentence -
        // the contract `docs/serving.md` states. (The 401 above is the bearer gate's, whose `code`
        // is top-level; this is the semantic refusal's, nested under its `reason`.)
        assert!(
            refused_body["reason"]["code"].is_string() && !refused_body["reason"]["code"].as_str().unwrap_or("").is_empty(),
            "{}",
            refused.body
        );
        assert!(refused_body["reason"]["detail"].is_string(), "{}", refused.body);

        // Ask 3: principal B, the same certified question - a different subject, so the audit
        // record it produces has to name a different subject too (the two-subject property
        // `nix/keycloak-tier.nix`'s `subjects` list provisions for).
        let reply_b = deployment.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&fixture.subject_b_token),
            QUESTION,
        );
        assert_eq!(reply_b.status, 200, "{}", reply_b.body);
        // `log()` drains the deployment's channel: this read returns the lines since the last read,
        // and the last read was after `finish` (ask A) - so this associates reply B with its own
        // record. Records arrive with their answers (never only at teardown), so a post-reply read
        // is enough.
        let lines_b = deployment.log();
        let subject_b = lines_b
            .iter()
            .rev()
            .find(|line| line.contains(RECORD))
            .map(|line| subject_field(line))
            .expect("ask B produced an audit record carrying its subject");
        assert_eq!(
            subject_b,
            mask(&sub_b),
            "B's record does not carry the mask of B's own token's subject"
        );
        // Uniqueness is `sub_a != sub_b` above (full `sub`s, not their one-hex masks, which the
        // audit's masking lets collide 1 in 16): each of the two distinct provisioned subjects'
        // tokens produced a record carrying that subject's OWN mask, so no record was attributed
        // to the wrong principal.
    }

    // The exchanged-identity half of the wave, as its own cell so its dependency is legible. **It
    // is NOT invoked by the task and this file keeps it `#[ignore]`d behind the maintainer's
    // binding, saying so** - it is exactly the leg `docs/where-identity-is-proven.md` keeps `unrun`,
    // because the shipped exchange yields only a federated workload-identity pool subject and there
    // is no `iamcredentials` hop to a concrete service account (#376 P2). Written so the `unrun`
    // row has a name its prose cites, and so a future binding has the assertion already shaped.
    #[test]
    #[ignore = "behind the maintainer's binding (#376 P2): SUTURA_BQ_PRINCIPAL_*_ASSERTION minted per subject and an iamcredentials hop to a service account"]
    fn the_source_executes_as_the_asking_subject() {
        // Reads `SESSION_USER()` the way `crates/sutura-exec-bigquery/tests/exchanged_identity.rs`
        // does, over a `bigquery` source under the catalog's own name with `posture:
        // impersonation-at-source`. The deployment-level credential plus the per-subject exchange
        // is what #376 P2 wires; until then each principal's assertion is absent from `bq-test` and
        // this cell - reached only by name - fails closed on the missing ones rather than silently
        // claiming to have executed as the subject.
        for (key, what) in [
            (
                "SUTURA_BQ_PRINCIPAL_A_EMAIL",
                "the account principal A's exchange must resolve to",
            ),
            ("SUTURA_BQ_PRINCIPAL_B_EMAIL", "the same for principal B"),
        ] {
            match std::env::var(key) {
                Ok(value) if !value.trim().is_empty() => drop(value),
                _ => panic!(
                    "{key} is not set - it names {what}. This is an acceptance leg behind the \
                     maintainer's binding (#376 P2): it needs a real subject-per-principal exchange, \
                     and its absence means the job cannot mean what it claims. See \
                     `docs/where-identity-is-proven.md`'s `unrun` row"
                ),
            }
        }
        // The env reads above are the fail-closed half: reached only by name, they refuse when the
        // per-subject assertions are absent (as they are from `bq-test` today), so a run never
        // silently claims to have executed as the subject over nothing. When they ARE present, this
        // cell still refuses - the actual `SESSION_USER()`-over-`iamcredentials` exchange (#376 P2)
        // is not wired in the served path, so a green here would be a green over code that does not
        // exist. The panic is the honest shape: this cell cannot pass until the binding lands.
        panic!(
            "// requires #376 P2: the iamcredentials hop and the SESSION_USER()-over-the-served- \
             path assertion are not wired; a green here before they land would overstate \
             docs/where-identity-is-proven.md's `unrun` row"
        );
    }
}
