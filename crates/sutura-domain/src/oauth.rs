//! OAuth 2.0 values a declared setting carries and a client sends: one grammar for each, so a
//! settings file is held at load to the rule the request is built with.

/// Why a declared scope list is unusable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidScope {
    /// Empty, or carrying a space, a `"` or a `\`, or a byte outside printable ASCII
    /// (RFC 6749 section 3.3's `scope-token`).
    #[error("the scope {given:?} is not an OAuth 2.0 scope token")]
    NotAScopeToken { given: String },
}

/// The scopes a client-credentials grant asks for, space-joined. Empty asks for none, and the
/// identity provider's default applies.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scopes(String);

impl Scopes {
    /// # Errors
    ///
    /// [`InvalidScope`], naming the first entry that is not a scope token.
    pub fn parse<'a>(scopes: impl IntoIterator<Item = &'a str>) -> Result<Self, InvalidScope> {
        let mut joined = String::new();
        for scope in scopes {
            let token = !scope.is_empty()
                && scope
                    .bytes()
                    .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\');
            if !token {
                return Err(InvalidScope::NotAScopeToken {
                    given: String::from(scope),
                });
            }
            if !joined.is_empty() {
                joined.push(' ');
            }
            joined.push_str(scope);
        }
        Ok(Self(joined))
    }

    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
