//! A source that runs each question under the asker's end-user security context, against a fake
//! listener that completes a password login and closes each session at its first message after it.

use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use sutura_conformance::corpus;
use sutura_dev::tns_listener::logging_in;
use sutura_domain::identity::{Presented, Secret};
use sutura_domain::plan::Executable;
use sutura_domain::warehouse::{ResultBudget, Warehouse as _};

use crate::{Channel, Dial, OracleError, OracleWarehouse};

/// The first asker's token, which no other byte of a session resembles.
const FIRST_ASKER: &str = "first-asker-7c1e9a";

/// The second asker's token, sharing no bytes with [`FIRST_ASKER`].
const SECOND_ASKER: &str = "second-asker-4d2b80";

/// A fake listener's side of a TLS session.
type Tls = rustls::StreamOwned<rustls::ServerConnection, TcpStream>;

/// A self-signed leaf for `127.0.0.1`, so its own PEM is the anchor that admits it.
fn issue() -> (rcgen::Certificate, rcgen::KeyPair) {
    let params = rcgen::CertificateParams::new([String::from("127.0.0.1")]).expect("an IP name parameterizes");
    let key = rcgen::KeyPair::generate().expect("a key pair generates");
    let certificate = params.self_signed(&key).expect("a self-signed leaf signs");
    (certificate, key)
}

/// The opener that presents `certificate` on every connection a fake listener accepts.
fn tls_for_each(
    certificate: &rcgen::Certificate,
    key: &rcgen::KeyPair,
) -> impl Fn(TcpStream) -> Option<Tls> + Clone + Send + 'static {
    let chain: Vec<CertificateDer<'static>> = vec![certificate.der().clone()];
    let private = PrivateKeyDer::try_from(key.serialize_der()).expect("a generated key is a private key");
    let config = Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("the default protocol versions are safe")
            .with_no_client_auth()
            .with_single_cert(chain, private)
            .expect("a generated chain and its own key are a pair"),
    );
    move |mut stream: TcpStream| {
        let mut server = rustls::ServerConnection::new(Arc::clone(&config)).ok()?;
        while server.is_handshaking() {
            let _io = server.complete_io(&mut stream).ok()?;
        }
        Some(rustls::StreamOwned::new(server, stream))
    }
}

/// A source that runs each question under the asker's end-user context, over `channel` to `port`.
fn attach_to(port: u16, channel: Channel<'_>) -> Result<OracleWarehouse, OracleError> {
    let dial = Dial::new("127.0.0.1", port, "FREEPDB1", channel).within(Duration::from_secs(5));
    let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024).expect("a test budget is positive"));
    OracleWarehouse::attaching(corpus::source(), dial, "sutura", "not-a-real-password", budget)
}

/// One question from the asker holding `token`, which the fake answers by closing its session.
fn ask(warehouse: &OracleWarehouse, token: &str) -> OracleError {
    let case = corpus::leg_case();
    let presented = Presented::SubjectToken {
        material: Secret::new(token),
    };
    warehouse
        .execute(Executable::Leg(case.leg()), &presented, corpus::deadline())
        .map(drop)
        .expect_err("the fake answers no statement")
}

/// How often `needle` occurs in `haystack`.
fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack.windows(needle.len()).filter(|window| *window == needle).count()
}

/// Whether the last message of a session the fake served is a statement over the corpus table.
fn ran_a_statement(sent: &[Vec<u8>]) -> bool {
    sent.last()
        .is_some_and(|last| occurrences(last, corpus::table().as_str().as_bytes()) >= 1)
}

/// **An attached question opens a session of its own, logged in as the declared user, and its
/// statement carries the asker's token in both fields of its end-user context.** The login sends no
/// token, the refusal - the fake closes the session at the statement - repeats none, and the boot
/// connection carries none.
#[test]
fn an_attached_question_opens_its_own_session_carrying_the_askers_token() {
    let (certificate, key) = issue();
    let pem = certificate.pem();
    let (port, served) = logging_in(tls_for_each(&certificate, &key), true).expect("the fake listener binds");
    let warehouse = attach_to(port, Channel::Verified { anchors_pem: &pem }).expect("the fake offers end-user contexts");
    let refused = ask(&warehouse, FIRST_ASKER);
    let question = served
        .recv_timeout(Duration::from_secs(10))
        .expect("the question's own session ended");
    let (login, statement) = question.split_at(question.len().saturating_sub(1));
    let statement = statement.first().expect("the session sent a message after its login");
    assert!(ran_a_statement(&question), "the session ran no statement");
    assert_eq!(occurrences(statement, b"ORCL_XS_AUTHZ_CONTEXT"), 1, "one end-user context");
    assert_eq!(
        occurrences(statement, FIRST_ASKER.as_bytes()),
        2,
        "the asker's token is the end-user token and the database-access token"
    );
    assert!(
        login.iter().any(|packet| occurrences(packet, b"sutura") >= 1),
        "the login names the declared user"
    );
    assert!(
        login.iter().all(|packet| occurrences(packet, FIRST_ASKER.as_bytes()) == 0),
        "the login sent the token"
    );
    let mut rendered = vec![format!("{refused:?}")];
    let mut cause: Option<&dyn std::error::Error> = Some(&refused);
    while let Some(error) = cause {
        rendered.push(error.to_string());
        cause = error.source();
    }
    let rendered = rendered.join(" | ");
    assert!(!rendered.contains(FIRST_ASKER), "the refusal repeats the token: {rendered}");
    drop(warehouse);
    let boot = served
        .recv_timeout(Duration::from_secs(10))
        .expect("the boot connection ended");
    assert!(!ran_a_statement(&boot), "a question ran on the boot connection");
    assert!(
        boot.iter().all(|packet| occurrences(packet, FIRST_ASKER.as_bytes()) == 0),
        "the boot connection carried the asker's token"
    );
}

/// **Two askers in turn never share a session, so the second never carries the first's context.**
/// Each question is its own session, ended before the next one opens, and the boot connection
/// carries neither asker. A source that ran both on one connection - setting each context on the
/// boot connection, with or without clearing it, or keeping one session for every question - serves
/// one statement here, not two.
#[test]
fn a_second_asker_never_shares_a_session_or_a_context_with_the_first() {
    let (certificate, key) = issue();
    let pem = certificate.pem();
    let (port, served) = logging_in(tls_for_each(&certificate, &key), true).expect("the fake listener binds");
    let warehouse = attach_to(port, Channel::Verified { anchors_pem: &pem }).expect("the fake offers end-user contexts");
    for token in [FIRST_ASKER, SECOND_ASKER] {
        let _refused = ask(&warehouse, token);
    }
    drop(warehouse);
    let mut sessions = Vec::new();
    while let Ok(sent) = served.recv_timeout(Duration::from_secs(10)) {
        sessions.push(sent);
        if sessions.len() == 3 {
            break;
        }
    }
    let (questions, boot): (Vec<_>, Vec<_>) = sessions.into_iter().partition(|sent| ran_a_statement(sent));
    let tokens: Vec<_> = questions
        .iter()
        .filter_map(|sent| sent.last())
        .map(|last| {
            (
                occurrences(last, FIRST_ASKER.as_bytes()),
                occurrences(last, SECOND_ASKER.as_bytes()),
            )
        })
        .collect();
    assert_eq!(tokens, vec![(2, 0), (0, 2)], "each asker's statement on a session of its own");
    assert_eq!(boot.len(), 1, "one boot connection");
    assert!(
        boot.iter()
            .flatten()
            .all(|packet| occurrences(packet, b"ORCL_XS_AUTHZ_CONTEXT") == 0),
        "the boot connection carried an end-user context"
    );
}

/// **A database that does not offer end-user contexts is refused at boot**, before any question, and
/// the boot connection sends no context.
#[test]
fn a_database_without_end_user_contexts_is_refused_before_any_question() {
    let (certificate, key) = issue();
    let pem = certificate.pem();
    let (port, served) = logging_in(tls_for_each(&certificate, &key), false).expect("the fake listener binds");
    let refused = attach_to(port, Channel::Verified { anchors_pem: &pem }).map(drop);
    assert!(
        matches!(refused, Err(OracleError::EndUserContextUnsupported { .. })),
        "{refused:?}"
    );
    let boot = served
        .recv_timeout(Duration::from_secs(10))
        .expect("the boot connection ended");
    assert!(!ran_a_statement(&boot), "the boot connection ran a statement");
    assert!(
        boot.iter().all(|packet| occurrences(packet, b"ORCL_XS_AUTHZ_CONTEXT") == 0),
        "the boot connection sent an end-user context"
    );
    assert!(
        served.recv_timeout(Duration::from_secs(1)).is_err(),
        "a second session was opened"
    );
}

/// **An attaching source over plaintext is refused before it dials**, so the declared user's password
/// is never sent in the clear.
#[test]
fn an_attaching_source_over_plaintext_is_refused_before_it_dials() {
    let (port, served) = logging_in(Some, true).expect("the fake listener binds");
    let refused = attach_to(port, Channel::Plaintext).map(drop);
    assert!(matches!(refused, Err(OracleError::EndUserContextNeedsTls)), "{refused:?}");
    assert!(
        served.recv_timeout(Duration::from_secs(2)).is_err(),
        "the listener was dialled"
    );
}

/// The compose-only Oracle venue, run by `just oracle-acceptance` and the `oracle-tier` CI job.
#[cfg(feature = "fixtures")]
mod live {
    use std::path::Path;

    use sutura_conformance::corpus;
    use sutura_dev::provisioned;
    use sutura_domain::identity::{Presented, Secret};
    use sutura_domain::plan::Executable;
    use sutura_domain::warehouse::{ResultBudget, Warehouse as _};

    use crate::fixture::FixtureCredential;
    use crate::{Channel, Dial, OracleError, OracleWarehouse};

    /// **A database that trusts no issuer refuses an attached question rather than answering it as
    /// the declared user.** The tier's image accepts no OAuth 2.0 issuer - its start-up script fails
    /// the tier when the `OAUTH2` identity provider type stops being refused - so it offers end-user
    /// security contexts, checks the attached token against no provider, and refuses the statement
    /// with `ORA-52600`.
    #[test]
    #[ignore = "requires just oracle-acceptance"]
    fn an_attached_question_is_refused_by_a_database_that_trusts_no_issuer() {
        let endpoint = provisioned::here(Path::new(env!("CARGO_MANIFEST_DIR")), "oracle")
            .endpoint()
            .cloned()
            .expect("the named acceptance task requires an Oracle endpoint");
        let fixture = FixtureCredential::from_env().expect("the named task sets the fixture credential");
        let budget = ResultBudget::of_bytes(core::num::NonZeroUsize::new(1024 * 1024).expect("a mebibyte is positive"));
        let dial = Dial::new(
            endpoint.host(),
            endpoint.port(),
            "FREEPDB1",
            Channel::Verified {
                anchors_pem: fixture.anchors_pem(),
            },
        );
        #[expect(
            clippy::disallowed_methods,
            reason = "the fixture password's destination is the tier's login"
        )]
        let password = fixture.password().expose_secret();
        let warehouse = OracleWarehouse::attaching(corpus::source(), dial, fixture.user(), password, budget)
            .expect("the tier offers end-user security contexts");
        let case = corpus::cases()
            .into_iter()
            .find(|case| case.name() == "total-by-region-and-day")
            .expect("the committed corpus has the region-and-day case");
        let presented = Presented::SubjectToken {
            material: Secret::new("not-a-token-any-issuer-signed"),
        };
        let refused = warehouse.execute(Executable::Query(case.plan()), &presented, corpus::deadline());
        assert!(
            matches!(refused, Err(OracleError::Execute { ref cause }) if cause.to_string().contains("ORA-52600")),
            "a database with no issuer must refuse the attached context: {refused:?}"
        );
    }
}
