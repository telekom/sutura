#![forbid(unsafe_code)]
//! The HTTP transport against a loopback fake: what reaches the wire, and what an error carries.

#[cfg(test)]
mod tests {
    use sutura_domain::identity::Secret;
    use sutura_domain::warehouse::deadline::{Budget, Deadline};
    use sutura_exec_cube::CubeTransport as _;
    use sutura_exec_cube::http::{Endpoint, HttpCube, HttpCubeError, LOAD_PATH, META_PATH, ReadBounds};
    use sutura_http_client::test_support::{FakeServer, Scripted};

    const TOKEN: &str = "cube-token-canary-93ab";

    fn transport(server: &FakeServer) -> HttpCube {
        HttpCube::new(
            Endpoint::parse(&server.endpoint()).expect("the fake server's own endpoint is usable"),
            ReadBounds::parse(10, 1 << 20).expect("a positive timeout and cap are usable bounds"),
            None,
        )
    }

    fn token() -> Secret {
        Secret::new(String::from(TOKEN))
    }

    fn deadline() -> Deadline {
        Deadline::opened_at(
            std::time::Instant::now(),
            Budget::parse(core::time::Duration::from_secs(10)).expect("a budget"),
        )
    }

    fn chain(error: &HttpCubeError) -> String {
        let mut text = format!("{error} {error:?}");
        let mut cause = std::error::Error::source(error);
        while let Some(link) = cause {
            text.push_str(" | ");
            text.push_str(&link.to_string());
            cause = link.source();
        }
        text
    }

    #[test]
    fn a_query_is_posted_as_json_with_the_token_as_a_bearer() {
        let answer = serde_json::json!({ "data": [] });
        let server = FakeServer::start(vec![Scripted::ok(&answer)]);
        let query = serde_json::json!({ "measures": ["fct_subscription_monthly.active_subscriptions"] });
        let body = transport(&server)
            .load(&query, &token(), deadline())
            .expect("the fake answers");
        let requests = server.finish();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).expect("JSON"), answer);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].request_line(), format!("POST {LOAD_PATH} HTTP/1.1"));
        assert_eq!(requests[0].authorization(), Some(format!("Bearer {TOKEN}").as_str()));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(requests[0].body()).expect("the body is JSON"),
            serde_json::json!({ "query": query })
        );
    }

    #[test]
    fn a_query_cube_is_still_computing_is_asked_again() {
        let answer = serde_json::json!({ "data": [{ "a.b": "1" }] });
        let server = FakeServer::start(vec![
            Scripted::ok(&serde_json::json!({ "error": "Continue wait" })),
            Scripted::ok(&answer),
        ]);
        let body = transport(&server)
            .load(&serde_json::json!({}), &token(), deadline())
            .expect("the second answer is the result");
        let requests = server.finish();
        assert_eq!(requests.len(), 2);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&body).expect("JSON"), answer);
    }

    #[test]
    fn a_refusal_carries_neither_the_token_nor_cubes_text() {
        let server = FakeServer::start(vec![Scripted::status(
            403,
            &format!(r#"{{"error":"Invalid token {TOKEN}"}}"#),
        )]);
        let cube = transport(&server);
        let refused = cube.meta(&token(), deadline()).expect_err("a 403 is a refusal");
        let requests = server.finish();
        assert_eq!(requests[0].request_line(), format!("GET {META_PATH} HTTP/1.1"));
        assert!(matches!(refused, HttpCubeError::Refused { status: 403 }), "{refused:?}");
        assert!(cube.source_refused(&refused));
        let rendered = chain(&refused);
        assert!(!rendered.contains(TOKEN), "{rendered}");
        assert!(!rendered.contains("Invalid token"), "{rendered}");
    }
}
