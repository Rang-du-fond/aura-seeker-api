pub type Result<T> = std::result::Result<T, Error>;

type Cause = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("resource not found")]
    NotFound,
    #[error(transparent)]
    Invalid(#[from] validator::ValidationErrors),
    #[error("{0}")]
    Forbidden(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error(transparent)]
    Unexpected(Cause),
}

impl Error {
    pub fn unexpected(cause: impl Into<Cause>) -> Self {
        Self::Unexpected(cause.into())
    }
}
