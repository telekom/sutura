//! The one body every failure comes back as, and the one thing that turns a failure into a
//! response.
//!
//! # A refusal is not in here
//!
//! Worth stating first, because it is the distinction the whole surface turns on. A *refusal* - the
//! caller asked something they may not have - is a `200` carrying
//! [`crate::wire::OutcomeBody::Refusal`]. A *failure* is everything else: a body that is not a
//! question, a missing credential, a limit reached, a data system that did not answer. Only
//! failures reach this module.
//!
//! # What a failure body may say
//!
//! [`Failure::Internal`] carries no detail, ever. The text of an internal error is a path, a table
//! name, a column name or a driver message, and handing any of those to an unauthenticated caller
//! is a description of the deployment. The detail is logged and the response says the status and
//! nothing else. Every other variant's detail is either fixed text or a message about the caller's
//! own request.

use axum::response::{IntoResponse, Response};

/// Why a request could not be answered.
///
/// One enum rather than a status code chosen per call site: the status, the code and the sentence
/// come from the variant, so two handlers cannot answer the same situation with different numbers.
#[derive(Debug)]
pub enum Failure {
    /// A credential is required and was absent, malformed or wrong.
    ///
    /// One variant for all three, deliberately: telling a caller which of the three they got wrong
    /// is telling them whether the secret they tried was close.
    Unauthorized,
    /// The body is not a question. Carries a message naming the field.
    NotAQuestion { detail: String },
    /// The body is larger than the configured bound.
    ///
    /// Separate from [`Self::NotAQuestion`] even though both arrive as the same extractor
    /// rejection: a caller who sent something too big has a different thing to fix than one who
    /// sent the wrong shape, and only the status tells them which.
    TooLarge,
    /// Too many requests from this address, too quickly.
    RateLimited,
    /// The request took longer than the configured bound.
    Timeout,
    /// Something on our side went wrong. Carries nothing.
    Internal,
    /// The data system did not answer. Distinguished from [`Self::Internal`] because it is the one
    /// failure that is worth retrying, and a caller cannot tell from a 500.
    Unavailable,
    /// Every execution slot was taken for the whole admission window, so the question was shed.
    ///
    /// **`503` and not `429`, and the two say different things.** A `429` is "you personally asked
    /// too often", which is a claim about the caller - and the rate limiter already makes it, keyed
    /// on an address. This is "the service has no capacity right now", which is a claim about the
    /// deployment and is true whoever asked. A caller inside their own rate limit can reach this,
    /// and telling them to slow down would be advice they cannot act on.
    ///
    /// Shares the status with [`Self::Unavailable`] and not the code, because the two are retried
    /// the same way and diagnosed differently: one is a data system that is unwell, the other is
    /// this service being full. `code` is what a client branches on, and the pair of them is why
    /// the code exists at all.
    ///
    /// Carries how long to wait. See [`Failure::retry_after`].
    AtCapacity { retry_after_seconds: u64 },
}

impl Failure {
    const fn status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match *self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotAQuestion { .. } => StatusCode::BAD_REQUEST,
            Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Timeout => StatusCode::REQUEST_TIMEOUT,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unavailable | Self::AtCapacity { .. } => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// How many seconds to wait before asking again, where there is an honest answer.
    ///
    /// Only [`Self::AtCapacity`], and only because there the number is already known: it is the
    /// admission window, which the caller just spent waiting out. So the header tells them nothing
    /// about this deployment that they did not measure themselves, which is the test every value
    /// leaving this module has to pass.
    ///
    /// Deliberately absent from [`Self::Unavailable`] and from [`Self::RateLimited`]. Nothing here
    /// knows when a data system will come back, and a number invented for the header would be a
    /// promise; the limiter emits its own quota headers, which are computed from its state rather
    /// than guessed.
    const fn retry_after(&self) -> Option<u64> {
        match *self {
            Self::AtCapacity { retry_after_seconds } => Some(retry_after_seconds),
            Self::Unauthorized
            | Self::NotAQuestion { .. }
            | Self::TooLarge
            | Self::RateLimited
            | Self::Timeout
            | Self::Internal
            | Self::Unavailable => None,
        }
    }

    /// The stable machine-readable code. What a client branches on.
    const fn code(&self) -> &'static str {
        match *self {
            Self::Unauthorized => "unauthorized",
            Self::NotAQuestion { .. } => "not_a_question",
            Self::TooLarge => "too_large",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Internal => "internal",
            Self::Unavailable => "unavailable",
            Self::AtCapacity { .. } => "at_capacity",
        }
    }

    /// The sentence a caller sees.
    fn detail(&self) -> String {
        match *self {
            Self::Unauthorized => String::from("this service requires a bearer token"),
            Self::NotAQuestion { ref detail } => detail.clone(),
            Self::TooLarge => String::from("the body is larger than this service will read"),
            Self::RateLimited => String::from("too many requests; slow down and retry"),
            Self::Timeout => String::from("the request exceeded this service's time bound"),
            // Fixed text. See the module documentation: the real detail is in the log.
            Self::Internal => String::from("this request could not be completed"),
            Self::Unavailable => String::from("the data system did not answer"),
            // No numbers. How many questions this deployment runs at once is its sizing, and a
            // caller who is being shed has no use for it - what they need is whether to retry,
            // which the status and `Retry-After` say.
            Self::AtCapacity { .. } => String::from("this service is at capacity; no question could be started in time. Retry"),
        }
    }

    /// The body, as a value a test can assert on without parsing a response.
    fn body(&self) -> ProblemBody {
        ProblemBody {
            code: self.code(),
            status: self.status().as_u16(),
            detail: self.detail(),
        }
    }
}

/// The failure body.
///
/// Three fields and no more. A request identifier would belong here and there is none: nothing in
/// this service mints one yet, and a field that is always absent is worse than no field.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
#[schema(as = ApiProblem)]
pub struct ProblemBody {
    /// A stable code, the same across every deployment and every version of this service.
    #[schema(example = "unauthorized")]
    code: &'static str,
    /// The HTTP status, repeated in the body so a client that logged only the body still has it.
    #[schema(example = 401)]
    status: u16,
    /// A sentence for a person. Never carries anything about this deployment's internals.
    detail: String,
}

impl ProblemBody {
    #[inline]
    pub const fn code(&self) -> &'static str {
        self.code
    }
}

impl IntoResponse for Failure {
    /// One body shape for every failure, plus a `Retry-After` where there is an honest number.
    ///
    /// Two arms rather than an always-present header with a sentinel value: `Retry-After: 0` is a
    /// promise that the next request will be answered, and a header that is sometimes a guess is
    /// worse than one that is sometimes absent.
    fn into_response(self) -> Response {
        let status = self.status();
        let body = axum::Json(self.body());
        match self.retry_after() {
            Some(seconds) => (status, [(axum::http::header::RETRY_AFTER, seconds.to_string())], body).into_response(),
            None => (status, body).into_response(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Failure;

    #[test]
    fn an_internal_failure_says_nothing_about_the_deployment() {
        // The security property of this module. An internal error's text is a path, a table name or
        // a driver message, and any of those handed to a caller describes the deployment.
        let body = Failure::Internal.body();
        assert_eq!(body.code(), "internal");
        let rendered = serde_json::to_string(&body).expect("the body serializes");
        assert_eq!(
            rendered,
            r#"{"code":"internal","status":500,"detail":"this request could not be completed"}"#
        );
    }

    #[test]
    fn an_unauthorized_failure_does_not_say_which_part_was_wrong() {
        // Absent, malformed and wrong are one variant on purpose: distinguishing them tells a
        // caller whether the secret they tried was close.
        assert_eq!(Failure::Unauthorized.body().code(), "unauthorized");
        assert_eq!(Failure::Unauthorized.status().as_u16(), 401);
    }

    #[test]
    fn every_failure_has_a_distinct_code_and_a_status_that_matches_its_kind() {
        let failures = [
            Failure::Unauthorized,
            Failure::NotAQuestion {
                detail: String::from("`grain` is not a grain"),
            },
            Failure::TooLarge,
            Failure::RateLimited,
            Failure::Timeout,
            Failure::Internal,
            Failure::Unavailable,
            Failure::AtCapacity { retry_after_seconds: 5 },
        ];
        let mut codes: Vec<&str> = failures.iter().map(Failure::code).collect();
        let count = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), count, "two failures share a code");
        for failure in &failures {
            // A failure that came back as a success status would be reported as an answer by every
            // client library there is.
            assert!(failure.status().is_client_error() || failure.status().is_server_error());
            assert_eq!(failure.body().status, failure.status().as_u16());
        }
    }

    #[test]
    fn being_full_and_a_data_system_being_down_share_a_status_and_not_a_code() {
        // Both are `503` and both are worth retrying, which is why the status is the same. They are
        // diagnosed in completely different places, which is why the code is not: one is this
        // service having no capacity, the other is a data system that did not answer, and a client
        // - or an operator reading a dashboard - branches on the code.
        let full = Failure::AtCapacity { retry_after_seconds: 5 };
        assert_eq!(full.status(), Failure::Unavailable.status());
        assert_ne!(full.code(), Failure::Unavailable.code());
        assert_eq!(full.code(), "at_capacity");
        // And the sentence carries no number: how many questions this deployment runs at once is
        // its sizing.
        let detail = full.detail();
        assert!(!detail.contains(|c: char| c.is_ascii_digit()), "{detail}");
    }

    #[test]
    fn only_a_shed_request_carries_a_retry_after_and_it_is_the_window_the_caller_already_waited() {
        // The rule for this header: a number that is already known, or no header. Anything else is
        // a promise about when a data system will come back, which nothing here knows.
        assert_eq!(Failure::AtCapacity { retry_after_seconds: 5 }.retry_after(), Some(5));
        for quiet in [
            Failure::Unauthorized,
            Failure::TooLarge,
            Failure::RateLimited,
            Failure::Timeout,
            Failure::Internal,
            Failure::Unavailable,
        ] {
            assert_eq!(quiet.retry_after(), None, "{quiet:?} invented a retry hint");
        }
    }

    #[test]
    fn a_malformed_question_carries_the_message_that_names_the_field() {
        // The one variant that reflects text, and the text is about the caller's own request.
        let failure = Failure::NotAQuestion {
            detail: String::from("`grain` is not one of: day, week, month, quarter, year"),
        };
        assert!(failure.detail().contains("grain"), "{}", failure.detail());
    }
}
