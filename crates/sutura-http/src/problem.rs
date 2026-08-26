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
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
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
    fn into_response(self) -> Response {
        (self.status(), axum::Json(self.body())).into_response()
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
    fn a_malformed_question_carries_the_message_that_names_the_field() {
        // The one variant that reflects text, and the text is about the caller's own request.
        let failure = Failure::NotAQuestion {
            detail: String::from("`grain` is not one of: day, week, month, quarter, year"),
        };
        assert!(failure.detail().contains("grain"), "{}", failure.detail());
    }
}
