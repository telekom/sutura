//! The served-caller proof: two independently-verified callers, through `sutura serve`, ADBC, and
//! two declared source principals - `docs/where-identity-is-proven.md`'s "a served binary under a
//! verified human caller" row, the half `crates/sutura-exec-bigquery/tests/declared_principal.rs`
//! could not reach because it drives the adapter directly.
//!
//! # Why this needed a SECOND identity provider, and why that provider is here rather than Google
//!
//! Leg 1 (verifying a caller) and leg 2 (federating that caller's assertion at the declared pool)
//! read the SAME token out of the same `Authorization` header. This surface's capability gate
//! (`sutura_http::capability`) refuses a caller whose token carries no `scope` claim, and a
//! Google-issued ID token has no scope claim to carry - so a Google-minted assertion satisfies the
//! pool and never reaches this surface at all. `nix/served-proof-tier.nix` is a Keycloak realm built
//! to satisfy both at once: it signs with a FIXED key whose public half a second Google
//! workload-identity provider was given directly (as `jwks_json`), so the SAME token this
//! deployment's gate accepts is one Google's STS can verify without ever reaching this loopback
//! realm - see that module's own header for the mechanism and where it was verified live.
//!
//! # What this file asserts, and the limit next to each half
//!
//! * **Two callers, two declared accounts, exact equality.** `ACCEPTS_RAW_STATEMENTS = false`
//!   keeps `SELECT SESSION_USER()` off `/v1/query` (`identity/SKILL.md`'s own note), so the account
//!   each question executed as is read the only way the served surface can show it: the ROW each
//!   caller's answer carries is filtered by a `BigQuery` row access policy granted to the ACCOUNT
//!   this source declares that caller executes as. The assertion is `assert_eq!` against the
//!   grouping value the deployment's own pulumi stack declares for that account
//!   (`SUTURA_BQ_PRINCIPAL_A_ROWS`/`_B_ROWS`), not merely that the two answers differ - the same
//!   upgrade `telekom/sutura#929` F3's re-review made to `declared_principal.rs`.
//! * **An undeclared subject is refused before anything is exchanged.** A third, verified caller
//!   with no entry in this source's `impersonate` map gets the same `credential_unavailable` this
//!   surface answers any impersonating source it holds nothing for, never a widened answer under a
//!   shared identity.
//! * **A deployment-identity control.** A SECOND `shared-service-user` source over the SAME
//!   policied table, asked by either caller, has to come back with no rows at all: the deployment's
//!   own credential is not a grantee of either row access policy, so if a query ever ran under it
//!   instead of a caller's federated principal the answer would carry every row this fixture seeds
//!   rather than one.
//!
//! What it does NOT establish: that Google's STS actually accepts this realm's token. That is the
//! HOSTED half, and `docs/where-identity-is-proven.md` records this venue as `wired` - the mechanism
//! is built and a job now reaches it, and no run has been observed yet.
//!
//! # Why it FAILS rather than skips
//!
//! Every environment value below is REQUIRED and a missing one panics naming it -
//! `declared_principal.rs`'s own header states why: a hosted venue whose environment is
//! half-configured must be RED, never a green run over nothing.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use base64::Engine as _;

    use crate::harness::{LOOPBACK, SINGLE_USER, config_path, derived_beside, settings_over, start_configured, v1};

    /// One required environment value, or a panic naming it - `declared_principal.rs`'s own
    /// `required`, restated here because that function is `pub(crate)` to a different crate.
    fn required(name: &str) -> String {
        match std::env::var(name) {
            Ok(value) if !value.trim().is_empty() => value,
            _ => panic!("{name} is unset or empty, and this venue proves nothing without it"),
        }
    }

    /// The one absolute path a served ADBC deployment needs for its `credential_file` key.
    /// Never opened by the transport for an impersonating request - the ADC it names is what the
    /// SHARED control source authenticates with - but `sutura_config` requires the key on every
    /// `bigquery` entry regardless of posture.
    fn credential_file() -> String {
        required("GOOGLE_APPLICATION_CREDENTIALS")
    }

    /// The bigquery source declared `impersonation-at-source`, over the RLS-policied table two
    /// verified callers are declared to execute against as two different accounts.
    const IMPERSONATING_SOURCE: &str = "warehouse";

    /// The bigquery source declared `shared-service-user`, over the SAME table - the control leg's
    /// own source, so its answer is the deployment's identity and nothing a caller supplied.
    const SHARED_SOURCE: &str = "warehouse_shared";

    /// The one custom Keycloak audience mapper this tier's own header explains: `security.inbound
    /// .resource` refuses anything that is not an absolute `https://` URI, so the pool's own
    /// `//iam.googleapis.com/...` audience cannot be reused as this surface's resource - read off
    /// the realm file the tier wrote, never restated as a literal here.
    struct ServedProofRealm {
        issuer: String,
        resource: String,
        capability_scope: String,
        callers: Vec<(String, String)>,
        client_id: String,
        client_secret: String,
        base: String,
    }

    fn read_realm(root: &Path) -> ServedProofRealm {
        let path = root.join(".sutura-dev/served-proof-realm.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|cause| panic!("{} is the served-proof tier's own realm file: {cause}", path.display()));
        let value: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|cause| panic!("{} is not valid JSON: {cause}", path.display()));
        let get = |key: &str| -> String {
            String::from(
                value
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("{} carries no `{key}`", path.display())),
            )
        };
        let client = value
            .get("client")
            .unwrap_or_else(|| panic!("{} carries no `client`", path.display()));
        let callers = value
            .get("callers")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("{} carries no `callers`", path.display()))
            .iter()
            .map(|entry| {
                let username = entry
                    .get("username")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("a served-proof caller entry names no `username`"));
                let password = entry
                    .get("password")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("a served-proof caller entry names no `password`"));
                (String::from(username), String::from(password))
            })
            .collect();
        let issuer = get("issuer");
        ServedProofRealm {
            base: issuer.trim_end_matches("/realms/served-proof").to_owned(),
            issuer,
            resource: get("resource"),
            capability_scope: get("capability_scope"),
            callers,
            client_id: String::from(
                client
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("{} carries no `client.id`", path.display())),
            ),
            client_secret: String::from(
                client
                    .get("secret")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_else(|| panic!("{} carries no `client.secret`", path.display())),
            ),
        }
    }

    /// A password-grant access token for one provisioned served-proof caller.
    fn mint(realm: &ServedProofRealm, username: &str, password: &str) -> String {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .build(),
        );
        let token_url = format!("{}/protocol/openid-connect/token", realm.issuer);
        let mut answer = agent
            .post(&token_url)
            .send_form([
                ("grant_type", "password"),
                ("client_id", realm.client_id.as_str()),
                ("client_secret", realm.client_secret.as_str()),
                ("username", username),
                ("password", password),
            ])
            .unwrap_or_else(|cause| panic!("the served-proof tier's own token endpoint at {token_url} is unreachable: {cause}"));
        let status = answer.status();
        let body = answer
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .unwrap_or_else(|cause| panic!("the token response is not readable: {cause}"));
        assert!(
            status.is_success(),
            "the served-proof tier refused a provisioned caller ({status}): {body}"
        );
        let value: serde_json::Value =
            serde_json::from_str(&body).unwrap_or_else(|cause| panic!("the token response is not JSON ({cause}): {body}"));
        String::from(
            value
                .get("access_token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("the token response carries no `access_token`: {value}")),
        )
    }

    /// One caller's declared account, one line of the `impersonate:` map.
    fn impersonate_line(caller: &str, account: &str) -> String {
        format!("      \"{caller}\": \"{account}\"\n")
    }

    /// The full `warehouse:` entry: `impersonation-at-source`, the pool this venue's tier tokens
    /// satisfy, and the two callers' full verified subjects mapped to the two accounts whose row
    /// grants already differ.
    fn impersonating_entry(caller_a: &str, account_a: &str, caller_b: &str, account_b: &str) -> String {
        let dataset = required("SUTURA_BQ_RLS_DATASET");
        format!(
            "  {IMPERSONATING_SOURCE}:\n    \
               kind: \"bigquery\"\n    \
               billing_project: \"{}\"\n    \
               dataset: \"{dataset}\"\n    \
               credential_file: \"{}\"\n    \
               max_bytes_billed: 1073741824\n    \
               posture: \"impersonation-at-source\"\n    \
               workload_identity:\n      \
                 audience: \"{}\"\n      \
                 scope: \"https://www.googleapis.com/auth/bigquery.readonly\"\n      \
                 impersonate:\n{}{}",
            required("SUTURA_BQ_PROJECT"),
            credential_file(),
            required("SUTURA_SERVED_PROOF_AUDIENCE"),
            impersonate_line(caller_a, account_a),
            impersonate_line(caller_b, account_b),
        )
    }

    /// The `warehouse_shared:` entry - the deployment's own credential, no caller in the chain.
    fn shared_entry() -> String {
        let dataset = required("SUTURA_BQ_RLS_DATASET");
        format!(
            "  {SHARED_SOURCE}:\n    \
               kind: \"bigquery\"\n    \
               billing_project: \"{}\"\n    \
               dataset: \"{dataset}\"\n    \
               credential_file: \"{}\"\n    \
               max_bytes_billed: 1073741824\n    \
               posture: \"shared-service-user\"\n    \
               acknowledged_because: \"the served-proof control leg reads this deployment's own identity, under no subject at all\"\n",
            required("SUTURA_BQ_PROJECT"),
            credential_file(),
        )
    }

    /// One markdown model plus one metric, over the fixed schema
    /// `test-infra/pulumi/google/__main__.py` seeds: `day` (DATE), `amount` (INT64), and a grouping
    /// column whose NAME is a pulumi config value, read here rather than assumed.
    fn write_model_and_metric(root: &Path, model_name: &str, source: &str) {
        let dataset = required("SUTURA_BQ_RLS_DATASET");
        let table = required("SUTURA_BQ_RLS_TABLE");
        let group_column = required("SUTURA_BQ_GROUP_COLUMN");
        let models = root.join("models");
        let metrics = root.join("metrics");
        std::fs::create_dir_all(&models).expect("the derived catalog's models directory is creatable");
        std::fs::create_dir_all(&metrics).expect("the derived catalog's metrics directory is creatable");
        std::fs::write(
            models.join(format!("{model_name}.md")),
            format!(
                "---\nkind: model\nname: {model_name}\nsource: {source}\ntable: {dataset}.{table}\ncolumns: [day, amount, {group_column}]\n---\nThe served-caller proof's own fixture table - not part of the quickstart catalog.\n"
            ),
        )
        .expect("the derived model document is writable");
        std::fs::write(
            metrics.join(format!("{model_name}_amount.md")),
            format!(
                "---\nkind: metric\nname: {model_name}_amount\nmodel: {model_name}\nmeasure:\n  simple: {{ aggregate: sum, column: amount }}\ntime_column: day\ngrains: [day]\ndimensions:\n  - name: {group_column}\n    column: {group_column}\naudience: open\n---\nThe served-caller proof's own metric - no anchor, so boot never re-executes it against a live table under an identity nothing has yet.\n"
            ),
        )
        .expect("the derived metric document is writable");
    }

    /// One caller's settings file: leg 1 declared against the served-proof tier, both bigquery
    /// sources, and a catalog naming both.
    fn settings(case: &str, realm: &ServedProofRealm, impersonating: &str, shared: &str) -> String {
        let catalog = derived_beside(&config_path(case));
        drop(std::fs::remove_dir_all(&catalog));
        write_model_and_metric(&catalog, "served_proof", IMPERSONATING_SOURCE);
        write_model_and_metric(&catalog, "served_proof_shared", SHARED_SOURCE);
        // Every model here is `bigquery`-backed, so nothing reads `data_dir` - but
        // `settings_over` still needs a path, and it has to be worktree-keyed rather than a
        // machine-shared one (`xtask check-worktree-state`'s own rule). The derived catalog
        // directory already is, and already exists.
        let data = catalog.clone();
        let security = format!(
            "  inbound:\n    mode: \"direct\"\n    resource: \"{}\"\n    \
             authorization_server: \"{}\"\n    key_set_file: \"{}\"\n    algorithms: [\"RS256\"]\n    \
             token_type: \"any\"\n",
            realm.resource,
            realm.issuer,
            fetch_key_set(realm, &catalog).display(),
        );
        settings_over(
            &catalog,
            &data,
            LOOPBACK,
            &format!("{SINGLE_USER}{security}"),
            &format!("{impersonating}{shared}"),
        )
    }

    /// The realm's own published key set, fetched over plain loopback HTTP and written for the
    /// deployment's `key_set_file` to read - the same split `served/harness/keycloak.rs` draws:
    /// the harness fetches, the binary never does.
    fn fetch_key_set(realm: &ServedProofRealm, scratch: &Path) -> PathBuf {
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .build(),
        );
        let jwks_url = format!("{}/realms/served-proof/protocol/openid-connect/certs", realm.base);
        let mut answer = agent
            .get(&jwks_url)
            .call()
            .unwrap_or_else(|cause| panic!("the served-proof tier's own JWKS endpoint at {jwks_url} is unreachable: {cause}"));
        let body = answer
            .body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_to_string()
            .unwrap_or_else(|cause| panic!("the key set response is not readable: {cause}"));
        let path = scratch.join("served-proof-jwks.json");
        std::fs::create_dir_all(scratch).expect("the derived catalog directory is creatable");
        std::fs::write(&path, body).expect("the fetched key set is writable");
        path
    }

    /// The `scope` claim a minted token carries, decoded from its own payload segment - the same
    /// read `served/harness/keycloak.rs::audience_of` performs for a different claim.
    fn scope_of(token: &str) -> String {
        let payload = token
            .split('.')
            .nth(1)
            .unwrap_or_else(|| panic!("{token} is not shaped like a compact JWT"));
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .unwrap_or_else(|cause| panic!("the token's payload segment is not base64url: {cause}"));
        let claims: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or_else(|cause| panic!("the token's payload segment is not JSON: {cause}"));
        String::from(
            claims
                .get("scope")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("the token carries no `scope`: {claims}")),
        )
    }

    /// The worktree this crate is inside - `declared_principal.rs`'s own reason for reading it
    /// rather than assuming the caller started the tier from here: a panic naming the task beats a
    /// connection refused thirty seconds in.
    fn worktree_root() -> PathBuf {
        sutura_dev::provisioned::worktree_root(Path::new(env!("CARGO_MANIFEST_DIR")))
            .expect("this crate is inside a checkout of the repository the served-proof tier provisions into")
    }

    #[test]
    #[ignore = "needs a real BigQuery project, the driver .so, and the served-proof Keycloak tier - run via `just served-proof-test`"]
    fn a_served_binary_executes_a_verified_human_caller_as_the_declared_account() {
        let root = worktree_root();
        let _endpoint = sutura_dev::provisioned::in_worktree(&root, "served-proof").unwrap_or_else(|absent| {
            panic!("{absent}\n  This cell is reached only by `just served-proof-test`, which brings the tier up first.")
        });
        let realm = read_realm(&root);
        assert!(
            realm.callers.len() >= 2,
            "the served-proof tier provisioned fewer than two callers - this venue needs two"
        );
        let (username_a, password_a) = &realm.callers[0];
        let (username_b, password_b) = &realm.callers[1];
        let token_a = mint(&realm, username_a, password_a);
        let token_b = mint(&realm, username_b, password_b);
        for token in [&token_a, &token_b] {
            let scope = scope_of(token);
            assert!(
                scope.split(' ').any(|granted| granted == realm.capability_scope),
                "a served-proof token carries no `{}` scope: {token}",
                realm.capability_scope
            );
        }

        let account_a = required("SUTURA_BQ_PRINCIPAL_A_EMAIL");
        let account_b = required("SUTURA_BQ_PRINCIPAL_B_EMAIL");
        let rows_a = required("SUTURA_BQ_PRINCIPAL_A_ROWS");
        let rows_b = required("SUTURA_BQ_PRINCIPAL_B_ROWS");
        let impersonating = impersonating_entry(username_a, &account_a, username_b, &account_b);
        let shared = shared_entry();

        let served = start_configured(
            "served-proof-e2e",
            &settings("served-proof-e2e", &realm, &impersonating, &shared),
        );

        let group_column = required("SUTURA_BQ_GROUP_COLUMN");
        let question = |metric: &str| {
            format!(
                r#"{{"metric":"{metric}","grain":"day","range":{{"start":"2026-01-01","end":"2026-01-03"}},"dimensions":["{group_column}"]}}"#
            )
        };

        // CALLER A, over the IMPERSONATING source: the row this account's own policy grants, and
        // no other - EXACT equality against the configured value, not merely a difference.
        let reply_a = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&token_a),
            &question("served_proof_amount"),
        );
        assert_eq!(reply_a.status, 200, "{}", reply_a.body);
        let groups_a = groups_of(&reply_a, &group_column);
        assert_eq!(groups_a, vec![rows_a], "caller A's own answer:\n{}", reply_a.body);

        // CALLER B, same question, same source: its own account's own row, and exact equality.
        let reply_b = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&token_b),
            &question("served_proof_amount"),
        );
        assert_eq!(reply_b.status, 200, "{}", reply_b.body);
        let groups_b = groups_of(&reply_b, &group_column);
        assert_eq!(groups_b, vec![rows_b], "caller B's own answer:\n{}", reply_b.body);

        assert_ne!(
            groups_a, groups_b,
            "both callers read the same row set - leg 2 did not federate two distinct principals"
        );

        // THE CONTROL: the SAME callers, the SHARED source over the SAME table - the deployment's
        // own credential is a grantee of neither row access policy, so this must come back empty.
        // If it carried EITHER `rows_a` or `rows_b`, a query ran under the deployment's own
        // identity rather than under whichever principal a caller's own assertion resolved to.
        let control = served.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&token_a),
            &question("served_proof_shared_amount"),
        );
        assert_eq!(control.status, 200, "{}", control.body);
        assert!(
            groups_of(&control, &group_column).is_empty(),
            "the deployment's own identity read a row neither caller's grant covers:\n{}",
            control.body
        );

        // AN UNDECLARED SUBJECT: caller B's OWN token, over a settings file whose impersonating
        // source names only caller A. Refused before anything is exchanged, never widened to the
        // deployment's identity.
        let undeclared_settings = settings(
            "served-proof-e2e-undeclared",
            &realm,
            &impersonating_entry(username_a, &account_a, username_a, &account_a),
            &shared,
        );
        let served_undeclared = start_configured("served-proof-e2e-undeclared", &undeclared_settings);
        let refused = served_undeclared.post(
            &v1(sutura_http::constants::base_paths::QUERY),
            Some(&token_b),
            &question("served_proof_amount"),
        );
        assert_eq!(refused.status, 403, "{}", refused.body);
        assert_eq!(refused.json()["code"], "credential_unavailable", "{}", refused.body);
    }

    /// The distinct values a caller's answer carries for `column`, as a sorted, deduplicated list -
    /// so a one-row and a many-row answer are compared the same way.
    fn groups_of(reply: &crate::harness::Reply, column: &str) -> Vec<String> {
        let json = reply.json();
        let rows = json["rows"]
            .as_array()
            .unwrap_or_else(|| panic!("no `rows` array: {}", reply.body));
        let columns = json["columns"]
            .as_array()
            .unwrap_or_else(|| panic!("no `columns` array: {}", reply.body));
        let at = columns
            .iter()
            .position(|name| name.as_str() == Some(column))
            .unwrap_or_else(|| panic!("`{column}` is not one of this answer's columns: {}", reply.body));
        let mut values: Vec<String> = rows
            .iter()
            .map(|row| {
                row.as_array()
                    .and_then(|cells| cells.get(at))
                    .and_then(|cell| cell.as_str())
                    .unwrap_or_else(|| panic!("row {row} carries no string at column {at}: {}", reply.body))
                    .to_owned()
            })
            .collect();
        values.sort();
        values.dedup();
        values
    }
}
