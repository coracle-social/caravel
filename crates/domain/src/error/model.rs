//! The core's error type, which says what went wrong in terms the API can map to
//! a response without knowing the operation that failed.

use caravel_util::db::Retryable;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    NotFound(&'static str),
    /// The resource is in a state that doesn't allow the operation.
    #[error("{message}")]
    Conflict { code: &'static str, message: String },
    /// The input is well-formed but not acceptable.
    #[error("{message}")]
    Invalid { code: &'static str, message: String },
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl Error {
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
        }
    }

    pub fn invalid(code: &'static str, message: impl Into<String>) -> Self {
        Self::Invalid {
            code,
            message: message.into(),
        }
    }

    pub fn internal(message: impl std::fmt::Display) -> Self {
        Self::Internal(anyhow::anyhow!("{message}"))
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        Self::Internal(error.into())
    }
}

impl Retryable for Error {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Internal(error) => error
                .downcast_ref::<sqlx::Error>()
                .is_some_and(Retryable::is_retryable),
            _ => false,
        }
    }
}
