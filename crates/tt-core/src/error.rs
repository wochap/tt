use thiserror::Error;

/// Domain errors. Each maps to a stable CLI exit code.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
    #[error("document error: {0}")]
    Document(String),
}

impl CoreError {
    /// 2 not found, 4 invalid state or conflict, 1 usage/parse.
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::NotFound(_) => 2,
            Self::Invalid(_) | Self::Conflict(_) | Self::Document(_) => 4,
        }
    }
}

impl From<automerge::AutomergeError> for CoreError {
    fn from(error: automerge::AutomergeError) -> Self {
        Self::Document(error.to_string())
    }
}

pub type CoreResult<T> = Result<T, CoreError>;
