use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

pub type Result<T> = std::result::Result<T, Error>;

type Cause = Box<dyn std::error::Error + Send + Sync>;

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    error: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge_id: Option<Uuid>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("wrong email or password")]
    InvalidCredentials,
    #[error("missing, malformed or expired access token")]
    InvalidToken,
    #[error("unknown, expired or already used refresh token")]
    InvalidRefreshToken,
    #[error("no active session with this id belongs to you")]
    UnknownSession,
    #[error("this sign-in token or nonce was not accepted")]
    InvalidIdToken,
    #[error("this account at the provider is already attached to another user")]
    IdentityLinkedElsewhere,
    #[error("this account at the provider must have the same verified email address as your account")]
    ProviderEmailMismatch,
    #[error("no sign-in method with this id belongs to you")]
    UnknownIdentity,
    #[error("this is your only way to sign in, add another one before removing it")]
    LastLoginMethod,
    #[error("your account at this provider has no verified email address")]
    ProviderEmailUnverified,
    #[error("this passkey was not accepted")]
    InvalidPasskey,
    #[error("no passkey with this id belongs to you")]
    UnknownPasskey,
    #[error("log in again to do this: your last login is too old")]
    StepUpRequired,
    #[error("too many requests, try again later")]
    RateLimited,
    #[error("wrong verification code")]
    InvalidCode,
    #[error("this verification code has expired or was already used, ask for a new one")]
    ChallengeExpired,
    #[error("too many wrong codes, ask for a new one")]
    TooManyAttempts,
    #[error("verify your email address with the code we just sent you")]
    EmailUnverified(Uuid),
    #[error("password must be 12 to 128 characters long")]
    InvalidPassword,
    #[error("this password appeared in a data breach, choose another one")]
    BreachedPassword,
    #[error("something went wrong on our side")]
    Unexpected(Cause),
}

impl Error {
    pub fn unexpected(cause: impl Into<Cause>) -> Self {
        Self::Unexpected(cause.into())
    }

    const fn code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::InvalidToken => (StatusCode::UNAUTHORIZED, "invalid_token"),
            Self::UnknownSession => (StatusCode::NOT_FOUND, "session_not_found"),
            Self::InvalidRefreshToken => (StatusCode::UNAUTHORIZED, "invalid_refresh_token"),
            Self::InvalidIdToken => (StatusCode::UNAUTHORIZED, "invalid_id_token"),
            Self::IdentityLinkedElsewhere => (StatusCode::CONFLICT, "identity_already_linked"),
            Self::ProviderEmailMismatch => (StatusCode::CONFLICT, "provider_email_mismatch"),
            Self::UnknownIdentity => (StatusCode::NOT_FOUND, "identity_not_found"),
            Self::LastLoginMethod => (StatusCode::CONFLICT, "last_login_method"),
            Self::ProviderEmailUnverified => (StatusCode::CONFLICT, "provider_email_unverified"),
            Self::InvalidPasskey => (StatusCode::UNAUTHORIZED, "invalid_passkey"),
            Self::UnknownPasskey => (StatusCode::NOT_FOUND, "passkey_not_found"),
            Self::StepUpRequired => (StatusCode::FORBIDDEN, "step_up_required"),
            Self::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::InvalidCode => (StatusCode::UNAUTHORIZED, "invalid_code"),
            Self::ChallengeExpired => (StatusCode::GONE, "challenge_expired"),
            Self::TooManyAttempts => (StatusCode::TOO_MANY_REQUESTS, "too_many_attempts"),
            Self::EmailUnverified(_) => (StatusCode::FORBIDDEN, "email_unverified"),
            Self::InvalidPassword => (StatusCode::UNPROCESSABLE_ENTITY, "invalid_password"),
            Self::BreachedPassword => (StatusCode::UNPROCESSABLE_ENTITY, "breached_password"),
            Self::Unexpected(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        if let Self::Unexpected(cause) = &self {
            tracing::error!(cause);
        }
        let (status, code) = self.code();
        let challenge_id = if let Self::EmailUnverified(challenge) = &self { Some(*challenge) } else { None };
        (status, Json(ErrorBody { error: code, message: self.to_string(), challenge_id })).into_response()
    }
}
