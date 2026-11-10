#![cfg(feature = "fixtures")]
#![forbid(unsafe_code)]
#![expect(
    clippy::disallowed_methods,
    reason = "a test dials the driver directly to observe the driver itself; test code does not ship"
)]

//! A login with a legacy password verifier, against the compose-only Oracle venue. `just
//! oracle-acceptance` and `nix run .#oracle-acceptance` (the `oracle-tier` CI job) start the tier
//! and run this ignored cell; the nix checks `just validate` runs execute no Oracle statement.
//!
//! **11G only.** The tier is Oracle AI Database 26ai, which has no 10G verifier, so no cell here
//! logs in with one: the vendored driver's own known-answer cells (`checks.vendored-oracledb`) are
//! the only proof of the 10G login.

#[cfg(test)]
mod acceptance {
    use std::path::Path;

    use sutura_dev::provisioned;

    /// The 11G verifier types a server names in the flags of `AUTH_VFR_DATA`.
    const VERIFIER_TYPES_11G: [u32; 2] = [0xb152, 0x1b25];

    /// The tier as this process reaches it: its address, the fixture password and its authority.
    struct Tier {
        address: String,
        password: String,
        anchors: String,
    }

    fn tier() -> Tier {
        let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
            .endpoint()
            .cloned()
            .expect("the named acceptance task requires an Oracle endpoint");
        Tier {
            address: format!("tcps://{}:{}/FREEPDB1", endpoint.host(), endpoint.port()),
            password: std::env::var("SUTURA_DEV_PASSWORD").expect("the acceptance task sets SUTURA_DEV_PASSWORD"),
            anchors: std::fs::read_to_string(
                std::env::var("SUTURA_ORACLE_TIER_CA").expect("the acceptance task sets SUTURA_ORACLE_TIER_CA"),
            )
            .expect("the tier's certificate authority is readable"),
        }
    }

    impl Tier {
        /// The driver configuration of a login as `user`, which may use a legacy verifier only
        /// when `legacy` is set.
        fn config(&self, user: &str, password: &str, legacy: bool) -> oracledb::Config {
            oracledb::Config::default()
                .set_connect_string(&self.address)
                .expect("the discovered endpoint is an Oracle connect string")
                .set_trust_anchors_pem(&self.anchors)
                .expect("the tier's certificate authority is an anchor")
                .set_credentials(user, password)
                .set_allow_legacy_verifiers(legacy)
        }

        fn login(&self, user: &str, password: &str, legacy: bool) -> Result<oracledb::Connection, oracledb::Error> {
            oracledb::connect(self.config(user, password, legacy))
        }
    }

    fn execute(connection: &oracledb::Connection, statement: &str) {
        connection
            .execute(statement, &[])
            .unwrap_or_else(|cause| panic!("{statement}: {cause}"));
    }

    /// Creates `user` with an 11G verifier and no other, and returns its password. The database
    /// computes the verifier, `S:` then SHA-1 over the password and the salt then the salt, so the
    /// login is checked against Oracle's own hash rather than the driver's. Dropped first where an
    /// earlier run left it, and never after: the tier is a throwaway one.
    fn legacy_11g_account(administrator: &oracledb::Connection, user: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past 1970")
            .subsec_nanos();
        let password = format!("P{:x}x{nanos:x}", std::process::id());
        let salt = format!("{:08X}{nanos:08X}A55A", std::process::id());
        let hash: String = administrator
            .query_row(
                "SELECT RAWTOHEX(STANDARD_HASH(UTL_RAW.CONCAT(UTL_RAW.CAST_TO_RAW(:1), HEXTORAW(:2)), 'SHA1')) FROM dual",
                &[&password.as_str(), &salt.as_str()],
            )
            .expect("the database hashes the password and the salt")
            .get(0)
            .expect("the hash is text");
        if let Err(cause) = administrator.execute(&format!("DROP USER \"{user}\" CASCADE"), &[]) {
            assert!(cause.to_string().contains("ORA-01918"), "{user} did not drop: {cause}");
        }
        execute(
            administrator,
            &format!("CREATE USER \"{user}\" IDENTIFIED BY VALUES 'S:{hash}{salt}'"),
        );
        execute(administrator, &format!("GRANT CREATE SESSION TO \"{user}\""));
        password
    }

    /// **An account with only an 11G verifier is refused by default, naming the verifier type,
    /// and logs in with the opt-in.** With the opt-in a wrong password is still `ORA-01017`, so the
    /// login is the database's check of the password and not the driver's leniency.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn an_account_with_only_an_11g_verifier_logs_in_only_with_the_opt_in() {
        let tier = tier();
        let administrator = tier
            .login("SYSTEM", &tier.password, false)
            .expect("the tier's administrator logs in with the 12C verifier");
        let user = format!("LEGACY_11G_{}", std::process::id());
        let password = legacy_11g_account(&administrator, &user);
        let versions: String = administrator
            .query_row(
                "SELECT password_versions FROM dba_users WHERE username = :1",
                &[&user.as_str()],
            )
            .expect("the dictionary records the account")
            .get(0)
            .expect("the password versions are text");
        assert_eq!(versions.trim(), "11G", "the account holds the 11G verifier alone");

        let Err(refused) = tier.login(&user, &password, false) else {
            panic!("an 11G login without the opt-in must be refused");
        };
        assert!(
            matches!(
                refused.kind(),
                oracledb::ErrorKind::PasswordVerifierRefused(found) if VERIFIER_TYPES_11G.contains(found)
            ),
            "the refusal names the 11G verifier type: {refused}"
        );
        assert!(refused.to_string().contains("(11G) is refused"), "{refused}");

        let Err(wrong) = tier.login(&user, "not-the-password", true) else {
            panic!("a wrong password must be refused with the opt-in too");
        };
        assert!(wrong.to_string().contains("ORA-01017"), "{wrong}");

        let session = tier
            .login(&user, &password, true)
            .expect("the 11G login succeeds with the opt-in");
        let current: String = session
            .query_row("SELECT USER FROM dual", &[])
            .expect("the session answers")
            .get(0)
            .expect("the current user is text");
        assert_eq!(current, user, "the session is the 11G account's");
    }
}
