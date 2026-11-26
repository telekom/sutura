//! `telekom/sutura#2082`'s `on_invalid_comment` policy.

/// What an unusable native table or column comment does, native-dictionary-only.
///
/// Not [`super::InvalidDescriptionPolicy`] - the two share a shape today, but a native comment has
/// no matched row or key source to fall back through, so nothing ties their variants together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommentPolicy {
    #[default]
    Refuse,
    Omit,
}

impl CommentPolicy {
    const ALL: [Self; 2] = [Self::Refuse, Self::Omit];

    pub(super) fn parse(written: &str) -> Option<Self> {
        let written = written.trim();
        Self::ALL.into_iter().find(|policy| policy.as_str() == written)
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Refuse => "refuse",
            Self::Omit => "omit",
        }
    }
}
