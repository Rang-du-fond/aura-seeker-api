use std::{
    convert::Infallible,
    future::{Future, ready},
    net::SocketAddr,
};

use axum::{
    Extension, Json,
    extract::{ConnectInfo, FromRequestParts, Path},
    http::{
        StatusCode,
        header::{AUTHORIZATION, USER_AGENT},
        request::Parts,
    },
};
use serde::{Deserialize, Serialize};
use utoipa::{
    ToSchema,
    openapi::security::{Http, HttpAuthScheme, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};
use uuid::Uuid;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use crate::{
    ActiveSession, Authenticator, Ceremony, Claims, Device, Error, LinkedIdentity, PasskeySummary, Result, Tokens,
    error::ErrorBody,
};

pub const SECURITY_SCHEME: &str = "bearer";

#[derive(Deserialize, ToSchema)]
struct Refresh {
    refresh_token: String,
}

#[derive(Deserialize, ToSchema)]
struct Credentials {
    email: String,
    password: String,
    device_label: Option<String>,
}

#[derive(Deserialize, ToSchema)]
struct EmailLogin {
    email: String,
}

#[derive(Deserialize, ToSchema)]
struct CodeVerification {
    challenge_id: Uuid,
    code: String,
    device_label: Option<String>,
}

#[derive(Deserialize, ToSchema)]
struct PasswordReset {
    challenge_id: Uuid,
    code: String,
    new_password: String,
}

#[derive(Deserialize, ToSchema)]
struct PasskeyRegistration {
    blob: String,
    #[schema(value_type = Object)]
    credential: RegisterPublicKeyCredential,
    label: Option<String>,
}

#[derive(Deserialize, ToSchema)]
struct PasskeyLogin {
    blob: String,
    #[schema(value_type = Object)]
    credential: PublicKeyCredential,
    device_label: Option<String>,
}

#[derive(Serialize, ToSchema)]
struct Nonce {
    nonce: String,
    expires_in: u64,
}

#[derive(Deserialize, ToSchema)]
struct GoogleLink {
    id_token: String,
    nonce: String,
}

#[derive(Deserialize, ToSchema)]
struct GoogleLogin {
    id_token: String,
    nonce: String,
    device_label: Option<String>,
}

#[derive(Serialize, ToSchema)]
struct ChallengeStarted {
    challenge_id: Uuid,
}

impl<S: Send + Sync> FromRequestParts<S> for Device {
    type Rejection = Infallible;

    fn from_request_parts(
        parts: &mut Parts,
        _: &S,
    ) -> impl Future<Output = std::result::Result<Self, Infallible>> + Send {
        let address = parts.extensions.get::<ConnectInfo<SocketAddr>>();
        ready(Ok(Self {
            label: None,
            user_agent: parts.headers.get(USER_AGENT).and_then(|value| value.to_str().ok()).map(Into::into),
            ip: address.map(|ConnectInfo(address)| address.ip().to_string()),
        }))
    }
}

pub struct AuthUser(pub Claims);

impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = Error;

    fn from_request_parts(parts: &mut Parts, _: &S) -> impl Future<Output = Result<Self>> + Send {
        ready(authenticated(parts))
    }
}

#[derive(Clone)]
struct AlreadyTraced;

fn authenticated(parts: &mut Parts) -> Result<AuthUser> {
    let authenticator = parts
        .extensions
        .get::<Authenticator>()
        .ok_or_else(|| Error::unexpected("the Authenticator extension is not installed"))?;
    let header = parts.headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok());
    let access_token = header.and_then(|value| value.strip_prefix("Bearer ")).ok_or(Error::InvalidToken)?;
    let claims = authenticator.authenticate(access_token)?;
    if parts.extensions.insert(AlreadyTraced).is_none() {
        tracing::Span::current().record("user.id", tracing::field::display(claims.sub));
    }
    Ok(AuthUser(claims))
}

#[utoipa::path(
    post, path = "/auth/password/signup", tag = "auth", request_body = Credentials,
    summary = "Create an account with a 12 to 128 character password; a 6-digit code is emailed to confirm the address",
    responses(
        (status = 202, body = ChallengeStarted, description = "Same answer whether the account was created or the email was already registered; send the code to /auth/email/verify"),
        (status = 422, body = ErrorBody, description = "invalid_password or breached_password"),
        (status = 429, body = ErrorBody, description = "rate_limited"),
    ),
)]
async fn signup(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(credentials): Json<Credentials>,
) -> Result<(StatusCode, Json<ChallengeStarted>)> {
    let challenge_id = authenticator.signup(&credentials.email, credentials.password, device.ip).await?;
    Ok((StatusCode::ACCEPTED, Json(ChallengeStarted { challenge_id })))
}

#[utoipa::path(
    post, path = "/auth/password/reset/start", tag = "auth", request_body = EmailLogin,
    summary = "Email a 6-digit code to reset the password of the account with this address",
    responses(
        (status = 202, body = ChallengeStarted, description = "Same answer whether or not an account exists"),
        (status = 429, body = ErrorBody, description = "rate_limited"),
    ),
)]
async fn start_password_reset(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(reset): Json<EmailLogin>,
) -> Result<(StatusCode, Json<ChallengeStarted>)> {
    let challenge_id = authenticator.start_password_reset(&reset.email, device.ip).await?;
    Ok((StatusCode::ACCEPTED, Json(ChallengeStarted { challenge_id })))
}

#[utoipa::path(
    post, path = "/auth/password/reset/complete", tag = "auth", request_body = PasswordReset,
    summary = "Set a new password with an emailed reset code; every device is signed out and no tokens are returned",
    responses(
        (status = 204, description = "Password changed, log in with it"),
        (status = 401, body = ErrorBody, description = "invalid_code"),
        (status = 410, body = ErrorBody, description = "challenge_expired"),
        (status = 422, body = ErrorBody, description = "invalid_password or breached_password"),
        (status = 429, body = ErrorBody, description = "too_many_attempts or rate_limited"),
    ),
)]
async fn complete_password_reset(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(reset): Json<PasswordReset>,
) -> Result<StatusCode> {
    authenticator.complete_password_reset(reset.challenge_id, &reset.code, reset.new_password, device.ip).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/auth/email/start", tag = "auth", request_body = EmailLogin,
    summary = "Email a 6-digit code to log in without a password; an unknown address gets an account once the code is verified",
    responses(
        (status = 202, body = ChallengeStarted, description = "Same answer whether or not an account exists"),
        (status = 429, body = ErrorBody, description = "rate_limited"),
    ),
)]
async fn start_email_login(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(login): Json<EmailLogin>,
) -> Result<(StatusCode, Json<ChallengeStarted>)> {
    let challenge_id = authenticator.start_email_login(&login.email, device.ip).await?;
    Ok((StatusCode::ACCEPTED, Json(ChallengeStarted { challenge_id })))
}

#[utoipa::path(
    post, path = "/auth/email/verify", tag = "auth", request_body = CodeVerification,
    summary = "Exchange an emailed code for tokens: confirms a signup, an unverified login or an email-code login",
    responses(
        (status = 200, body = Tokens),
        (status = 401, body = ErrorBody, description = "invalid_code"),
        (status = 410, body = ErrorBody, description = "challenge_expired"),
        (status = 429, body = ErrorBody, description = "too_many_attempts or rate_limited"),
    ),
)]
async fn verify_email_code(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(verification): Json<CodeVerification>,
) -> Result<Json<Tokens>> {
    let device = Device { label: verification.device_label, ..device };
    Ok(Json(authenticator.verify_email_code(verification.challenge_id, &verification.code, device).await?))
}

#[utoipa::path(
    post, path = "/auth/password/login", tag = "auth", request_body = Credentials,
    summary = "Log in with an email and a password; `device_label` optionally names the device, e.g. \"Pixel 9\"",
    responses(
        (status = 200, body = Tokens),
        (status = 401, body = ErrorBody, description = "invalid_credentials"),
        (status = 403, body = ErrorBody, description = "email_unverified: a code was emailed, send it with the returned challenge_id to /auth/email/verify"),
    ),
)]
async fn login(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(credentials): Json<Credentials>,
) -> Result<Json<Tokens>> {
    let device = Device { label: credentials.device_label, ..device };
    Ok(Json(authenticator.login(&credentials.email, credentials.password, device).await?))
}

#[utoipa::path(
    post, path = "/auth/refresh", tag = "auth", request_body = Refresh,
    summary = "Exchange a refresh token for a new access token and a new refresh token; the old one stops working",
    responses((status = 200, body = Tokens), (status = 401, body = ErrorBody, description = "invalid_refresh_token")),
)]
async fn refresh(
    Extension(authenticator): Extension<Authenticator>,
    Json(refresh): Json<Refresh>,
) -> Result<Json<Tokens>> {
    Ok(Json(authenticator.refresh(&refresh.refresh_token).await?))
}

#[utoipa::path(
    post, path = "/auth/logout", tag = "auth", security(("bearer" = [])),
    summary = "Sign out this device: its refresh token stops working",
    responses((status = 204, description = "Signed out"), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn logout(Extension(authenticator): Extension<Authenticator>, AuthUser(claims): AuthUser) -> Result<StatusCode> {
    authenticator.logout(&claims).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post, path = "/auth/logout-all", tag = "auth", security(("bearer" = [])),
    summary = "Sign out every device of the authenticated user, this one included",
    responses((status = 204, description = "Signed out everywhere"), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn logout_everywhere(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
) -> Result<StatusCode> {
    authenticator.logout_everywhere(&claims).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/auth/sessions", tag = "auth", security(("bearer" = [])),
    summary = "List the devices the authenticated user is signed in on",
    responses((status = 200, body = Vec<ActiveSession>), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn sessions(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<ActiveSession>>> {
    Ok(Json(authenticator.sessions(&claims).await?))
}

#[utoipa::path(
    delete, path = "/auth/sessions/{id}", tag = "auth", params(("id" = Uuid, Path)), security(("bearer" = [])),
    summary = "Sign out one device of the authenticated user",
    responses(
        (status = 204, description = "Signed out"),
        (status = 401, body = ErrorBody, description = "invalid_token"),
        (status = 404, body = ErrorBody, description = "session_not_found"),
    ),
)]
async fn revoke_session(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    authenticator.revoke_session(&claims, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/auth/me", tag = "auth", security(("bearer" = [])),
    summary = "Read the claims of the presented access token",
    responses((status = 200, body = Claims), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn me(AuthUser(claims): AuthUser) -> Json<Claims> {
    Json(claims)
}

#[must_use]
pub fn router(authenticator: Authenticator) -> OpenApiRouter {
    let mut router = login_routes()
        .merge(session_routes())
        .merge(passkey_routes())
        .merge(provider_routes())
        .layer(Extension(authenticator));
    let bearer = SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer));
    router.get_openapi_mut().components.get_or_insert_default().add_security_scheme(SECURITY_SCHEME, bearer);
    router
}

#[utoipa::path(
    post, path = "/auth/passkeys/register/start", tag = "auth", security(("bearer" = [])),
    summary = "Get WebAuthn creation options to add a passkey; needs a login less than 10 minutes old",
    responses(
        (status = 200, body = Ceremony, description = "`options` goes to navigator.credentials.create, `blob` comes back with the result"),
        (status = 401, body = ErrorBody, description = "invalid_token"),
        (status = 403, body = ErrorBody, description = "step_up_required"),
    ),
)]
async fn start_passkey_registration(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Ceremony>> {
    Ok(Json(authenticator.start_passkey_registration(&claims).await?))
}

#[utoipa::path(
    post, path = "/auth/passkeys/register/finish", tag = "auth", request_body = PasskeyRegistration,
    security(("bearer" = [])),
    summary = "Store the passkey created by the device",
    responses(
        (status = 201, body = PasskeySummary),
        (status = 401, body = ErrorBody, description = "invalid_token or invalid_passkey"),
        (status = 403, body = ErrorBody, description = "step_up_required"),
        (status = 410, body = ErrorBody, description = "challenge_expired"),
    ),
)]
async fn finish_passkey_registration(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
    Json(registration): Json<PasskeyRegistration>,
) -> Result<(StatusCode, Json<PasskeySummary>)> {
    let PasskeyRegistration { blob, credential, label } = registration;
    let passkey = authenticator.finish_passkey_registration(&claims, &blob, &credential, label).await?;
    Ok((StatusCode::CREATED, Json(passkey)))
}

#[utoipa::path(
    post, path = "/auth/passkeys/login/start", tag = "auth",
    summary = "Get WebAuthn request options to log in with a passkey, without typing an email",
    responses((status = 200, body = Ceremony, description = "`options` goes to navigator.credentials.get, `blob` comes back with the result")),
)]
async fn start_passkey_login(Extension(authenticator): Extension<Authenticator>) -> Result<Json<Ceremony>> {
    Ok(Json(authenticator.start_passkey_login()?))
}

#[utoipa::path(
    post, path = "/auth/passkeys/login/finish", tag = "auth", request_body = PasskeyLogin,
    summary = "Log in with the passkey assertion produced by the device",
    responses(
        (status = 200, body = Tokens),
        (status = 401, body = ErrorBody, description = "invalid_passkey"),
        (status = 410, body = ErrorBody, description = "challenge_expired"),
    ),
)]
async fn finish_passkey_login(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(login): Json<PasskeyLogin>,
) -> Result<Json<Tokens>> {
    let device = Device { label: login.device_label, ..device };
    Ok(Json(authenticator.finish_passkey_login(&login.blob, &login.credential, device).await?))
}

#[utoipa::path(
    get, path = "/auth/passkeys", tag = "auth", security(("bearer" = [])),
    summary = "List the passkeys of the authenticated user",
    responses((status = 200, body = Vec<PasskeySummary>), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn passkeys(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<PasskeySummary>>> {
    Ok(Json(authenticator.passkeys(&claims).await?))
}

#[utoipa::path(
    delete, path = "/auth/passkeys/{id}", tag = "auth", params(("id" = Uuid, Path)), security(("bearer" = [])),
    summary = "Remove a passkey; needs a login less than 10 minutes old",
    responses(
        (status = 204, description = "Removed"),
        (status = 401, body = ErrorBody, description = "invalid_token"),
        (status = 403, body = ErrorBody, description = "step_up_required"),
        (status = 404, body = ErrorBody, description = "passkey_not_found"),
    ),
)]
async fn remove_passkey(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    authenticator.remove_passkey(&claims, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    get, path = "/auth/nonce", tag = "auth",
    summary = "Get a signed nonce, valid 5 minutes, to pass to the Google sign-in dialog",
    responses((status = 200, body = Nonce)),
)]
async fn nonce(Extension(authenticator): Extension<Authenticator>) -> Result<Json<Nonce>> {
    let (nonce, expires_in) = authenticator.nonce()?;
    Ok(Json(Nonce { nonce, expires_in }))
}

#[utoipa::path(
    post, path = "/auth/google/token", tag = "auth", request_body = GoogleLogin,
    summary = "Log in with the ID token returned by the native Google sign-in dialog; creates or links the account",
    responses(
        (status = 200, body = Tokens),
        (status = 401, body = ErrorBody, description = "invalid_id_token"),
        (status = 409, body = ErrorBody, description = "provider_email_unverified"),
    ),
)]
async fn login_with_google(
    Extension(authenticator): Extension<Authenticator>,
    device: Device,
    Json(login): Json<GoogleLogin>,
) -> Result<Json<Tokens>> {
    let device = Device { label: login.device_label, ..device };
    Ok(Json(authenticator.login_with_google(&login.id_token, &login.nonce, device).await?))
}

#[utoipa::path(
    get, path = "/auth/identities", tag = "auth", security(("bearer" = [])),
    summary = "List the sign-in methods attached to the authenticated account: password and providers",
    responses((status = 200, body = Vec<LinkedIdentity>), (status = 401, body = ErrorBody, description = "invalid_token")),
)]
async fn identities(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
) -> Result<Json<Vec<LinkedIdentity>>> {
    Ok(Json(authenticator.identities(&claims).await?))
}

#[utoipa::path(
    post, path = "/auth/google/link", tag = "auth", request_body = GoogleLink, security(("bearer" = [])),
    summary = "Attach the Google account that has the same verified email as the authenticated user; needs a login less than 10 minutes old",
    responses(
        (status = 201, body = LinkedIdentity, description = "Attached"),
        (status = 200, body = LinkedIdentity, description = "Was already attached to this user"),
        (status = 401, body = ErrorBody, description = "invalid_token or invalid_id_token"),
        (status = 403, body = ErrorBody, description = "step_up_required"),
        (status = 409, body = ErrorBody, description = "identity_already_linked (attached to another user) or provider_email_mismatch"),
    ),
)]
async fn link_google(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
    Json(link): Json<GoogleLink>,
) -> Result<(StatusCode, Json<LinkedIdentity>)> {
    let (created, identity) = authenticator.link_google(&claims, &link.id_token, &link.nonce).await?;
    Ok((if created { StatusCode::CREATED } else { StatusCode::OK }, Json(identity)))
}

#[utoipa::path(
    delete, path = "/auth/identities/{id}", tag = "auth", params(("id" = Uuid, Path)), security(("bearer" = [])),
    summary = "Remove a sign-in method, unless it is the last one; needs a login less than 10 minutes old",
    responses(
        (status = 204, description = "Removed"),
        (status = 401, body = ErrorBody, description = "invalid_token"),
        (status = 403, body = ErrorBody, description = "step_up_required"),
        (status = 404, body = ErrorBody, description = "identity_not_found"),
        (status = 409, body = ErrorBody, description = "last_login_method"),
    ),
)]
async fn unlink_identity(
    Extension(authenticator): Extension<Authenticator>,
    AuthUser(claims): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    authenticator.unlink_identity(&claims, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn provider_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(nonce))
        .routes(routes!(login_with_google))
        .routes(routes!(link_google))
        .routes(routes!(identities))
        .routes(routes!(unlink_identity))
}

fn passkey_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(start_passkey_registration))
        .routes(routes!(finish_passkey_registration))
        .routes(routes!(start_passkey_login))
        .routes(routes!(finish_passkey_login))
        .routes(routes!(passkeys))
        .routes(routes!(remove_passkey))
}

fn login_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(signup))
        .routes(routes!(login))
        .routes(routes!(start_password_reset))
        .routes(routes!(complete_password_reset))
        .routes(routes!(start_email_login))
        .routes(routes!(verify_email_code))
        .routes(routes!(refresh))
}

fn session_routes() -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(routes!(logout))
        .routes(routes!(logout_everywhere))
        .routes(routes!(sessions))
        .routes(routes!(revoke_session))
        .routes(routes!(me))
}
