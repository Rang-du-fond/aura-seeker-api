use std::sync::Arc;

use jsonwebtoken::get_current_timestamp as now;
use notifier::{Notification, Notifier};
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;
use webauthn_rs::prelude::{PublicKeyCredential, RegisterPublicKeyCredential};

use crate::{
    Account, Claims, Error, Keyring, LinkedIdentity, Result, Settings,
    breach::Breaches,
    challenge::{self, Challenge, Codes},
    limiter::Limiter,
    nonce::{self, Nonces},
    notice,
    passkey::{Ceremony, PasskeyRecord, PasskeySummary, Passkeys},
    password,
    provider::{ExternalIdentity, OidcProvider},
    session::{self, ActiveSession, Device, RefreshToken, Session},
    storage::{Records, Storage, Store},
    token::PASSKEY_METHOD,
};

const PASSWORD_METHOD: &str = "pwd";
const CODE_METHOD: &str = "otp";
const PASSKEY_ID: &str = "id";
const PASSKEY_USER: &str = "user_id";
const PASSKEY_CREDENTIAL: &str = "credential_id";
const CHALLENGE_ID: &str = "id";
const CHALLENGE_EMAIL: &str = "email";
const CHALLENGE_USER: &str = "user_id";
const CHALLENGE_ATTEMPTS: &str = "attempts";
const CHALLENGE_END: &str = "expires_at";
const CHALLENGE_RETENTION: u64 = 24 * 3600;
const DEFAULT_ROLE: &str = "user";
const SESSION_ID: &str = "id";
const SESSION_USER: &str = "user_id";
const SESSION_END_FIELDS: [&str; 3] = ["revoked_at", "idle_expires_at", "absolute_expires_at"];
const TOKEN_HASH: &str = "token_hash";
const SUCCESSOR: &str = "replaced_by";

#[derive(Serialize, ToSchema)]
pub struct Tokens {
    access_token: String,
    token_type: &'static str,
    expires_in: u64,
    refresh_token: String,
    refresh_expires_in: u64,
    user: User,
}

#[derive(Serialize, ToSchema)]
struct User {
    id: Uuid,
    email: String,
}

#[derive(Clone)]
pub struct Authenticator {
    storage: Arc<dyn Storage>,
    keyring: Arc<Keyring>,
    codes: Arc<Codes>,
    breaches: Arc<Breaches>,
    webauthn: Arc<Passkeys>,
    code_sends: Arc<Limiter>,
    code_checks: Arc<Limiter>,
    google: Arc<OidcProvider>,
    nonces: Arc<Nonces>,
    notifier: Arc<dyn Notifier>,
    settings: Arc<Settings>,
}

impl Authenticator {
    pub fn new(storage: impl Storage + 'static, notifier: Arc<dyn Notifier>, settings: Settings) -> Result<Self> {
        Ok(Self {
            storage: Arc::new(storage),
            keyring: Arc::new(Keyring::load(&settings)?),
            codes: Arc::new(Codes::load(&settings)?),
            breaches: Arc::new(Breaches::configured(&settings)?),
            webauthn: Arc::new(Passkeys::configured(&settings)?),
            google: Arc::new(OidcProvider::google(&settings)?),
            nonces: Arc::new(Nonces::configured(&settings)?),
            code_sends: Arc::new(Limiter::new(
                settings.rate_limits.code_sends_per_ip,
                settings.rate_limits.code_sends_window,
            )),
            code_checks: Arc::new(Limiter::new(
                settings.rate_limits.code_checks_per_ip,
                settings.rate_limits.code_checks_window,
            )),
            notifier,
            settings: Arc::new(settings),
        })
    }

    #[tracing::instrument(name = "auth.signup", skip_all)]
    pub async fn signup(&self, email: &str, password: String, ip: Option<String>) -> Result<Uuid> {
        let email = normalized(email);
        password::check_policy(&password)?;
        self.admit_code_request(&email, ip.as_deref()).await?;
        let password_hash = Some(self.acceptable_hash(password).await?);
        let verification =
            Subject { email: email.clone(), purpose: challenge::VERIFY_EMAIL, user_id: None, ip, real: false };
        if self.storage.find_by_email(&email).await?.is_some() {
            self.notify_in_background(notice::account_exists(email));
            return self.open_challenge(&*self.storage, verification).await;
        }
        let account = Account { password_hash, ..new_account(email, false) };
        let verification = Subject { user_id: Some(account.id), real: true, ..verification };
        let transaction = self.storage.begin().await?;
        transaction.register(&account).await?;
        let challenge_id = self.open_challenge(&*transaction, verification).await?;
        transaction.commit().await?;
        Ok(challenge_id)
    }

    #[tracing::instrument(name = "auth.start_email_login", skip_all)]
    pub async fn start_email_login(&self, email: &str, ip: Option<String>) -> Result<Uuid> {
        let email = normalized(email);
        self.admit_code_request(&email, ip.as_deref()).await?;
        let earlier: Vec<(Uuid, Challenge)> = self.storage.matching(CHALLENGE_EMAIL, email.clone()).await?;
        for (id, pending) in
            earlier.into_iter().filter(|(_, pending)| pending.purpose == challenge::LOGIN && pending.is_open(now()))
        {
            self.storage.replace(id, pending.consumed(now())).await?;
        }
        let user_id = self.storage.find_by_email(&email).await?.map(|account| account.id);
        self.open_challenge(&*self.storage, Subject { email, purpose: challenge::LOGIN, user_id, ip, real: true }).await
    }

    #[tracing::instrument(name = "auth.verify_email_code", skip_all)]
    pub async fn verify_email_code(&self, challenge_id: Uuid, code: &str, device: Device) -> Result<Tokens> {
        self.code_checks.admit(device.ip.as_deref(), now())?;
        let attempted = self.attempt(challenge_id).await?;
        let grants_login = attempted.purpose != challenge::PASSWORD_RESET;
        if !(grants_login && self.codes.matches(challenge_id, code, &attempted.code_hash)?) {
            return Err(Error::InvalidCode);
        }
        let method = if attempted.purpose == challenge::LOGIN { CODE_METHOD } else { PASSWORD_METHOD };
        let account = self.confirm(challenge_id, attempted).await?;
        self.start_session(account, method, device).await
    }

    pub fn nonce(&self) -> Result<(String, u64)> {
        Ok((self.nonces.issue(now())?, nonce::LIFETIME))
    }

    #[tracing::instrument(name = "auth.login_with_google", skip_all)]
    pub async fn login_with_google(&self, id_token: &str, nonce: &str, device: Device) -> Result<Tokens> {
        self.nonces.check(nonce, now())?;
        let identity = self.google.verify_id_token(id_token, nonce).await?;
        let method = identity.provider;
        let account = self.resolve(identity).await?;
        self.start_session(account, method, device).await
    }

    #[tracing::instrument(name = "auth.identities", skip_all)]
    pub async fn identities(&self, claims: &Claims) -> Result<Vec<LinkedIdentity>> {
        self.storage.identities(claims.sub).await
    }

    #[tracing::instrument(name = "auth.link_google", skip_all)]
    pub async fn link_google(&self, claims: &Claims, id_token: &str, nonce: &str) -> Result<(bool, LinkedIdentity)> {
        self.ensure_recent_login(claims)?;
        self.nonces.check(nonce, now())?;
        let identity = self.google.verify_id_token(id_token, nonce).await?;
        let owner = self.storage.find_by_identity(identity.provider, &identity.subject).await?;
        if owner.as_ref().is_some_and(|owner| owner.id != claims.sub) {
            return Err(Error::IdentityLinkedElsewhere);
        }
        if owner.is_none() {
            let account = self.storage.find(claims.sub).await?.ok_or(Error::InvalidToken)?;
            let provider_email = identity.email.as_deref().filter(|_| identity.email_verified).map(normalized);
            if provider_email.as_deref() != Some(account.email.as_str()) {
                return Err(Error::ProviderEmailMismatch);
            }
            self.storage.link_identity(claims.sub, &identity).await?;
            self.notify_in_background(notice::provider_linked(account.email, identity.provider));
        }
        let mut linked = self.storage.identities(claims.sub).await?;
        linked.retain(|linked| linked.provider == identity.provider && linked.subject == identity.subject);
        Ok((owner.is_none(), linked.pop().ok_or(Error::UnknownIdentity)?))
    }

    #[tracing::instrument(name = "auth.unlink_identity", skip_all)]
    pub async fn unlink_identity(&self, claims: &Claims, id: Uuid) -> Result<()> {
        self.ensure_recent_login(claims)?;
        let identities = self.storage.identities(claims.sub).await?;
        if !identities.iter().any(|identity| identity.id == id) {
            return Err(Error::UnknownIdentity);
        }
        let passkeys = self.user_passkeys(claims.sub).await?.len();
        if identities.len() + passkeys <= 1 {
            return Err(Error::LastLoginMethod);
        }
        self.storage.unlink_identity(id).await
    }

    async fn resolve(&self, identity: ExternalIdentity) -> Result<Account> {
        if let Some(account) = self.storage.find_by_identity(identity.provider, &identity.subject).await? {
            return Ok(account);
        }
        let verified_email = identity.email.as_deref().filter(|_| identity.email_verified).map(normalized);
        let email = verified_email.ok_or(Error::ProviderEmailUnverified)?;
        let existing = self.storage.find_by_email(&email).await?;
        let transaction = self.storage.begin().await?;
        let account = if let Some(account) = existing {
            self.adopt(&*transaction, account, identity.provider).await?
        } else {
            let account = Account { display_name: identity.display_name.clone(), ..new_account(email, true) };
            transaction.register(&account).await?;
            account
        };
        transaction.link_identity(account.id, &identity).await?;
        transaction.commit().await?;
        Ok(account)
    }

    async fn adopt(&self, records: &dyn Records, account: Account, provider: &str) -> Result<Account> {
        if !account.email_verified {
            records.remove_password(account.id).await?;
            records.mark_email_verified(account.id).await?;
        }
        self.notify_in_background(notice::provider_linked(account.email.clone(), provider));
        Ok(Account { email_verified: true, ..account })
    }

    #[tracing::instrument(name = "auth.start_passkey_registration", skip_all)]
    pub async fn start_passkey_registration(&self, claims: &Claims) -> Result<Ceremony> {
        self.ensure_recent_login(claims)?;
        let account = self.storage.find(claims.sub).await?.ok_or(Error::InvalidToken)?;
        let existing: Vec<_> = self.user_passkeys(claims.sub).await?.into_iter().map(|(_, record)| record).collect();
        self.webauthn.start_registration(&account, &existing, now())
    }

    #[tracing::instrument(name = "auth.finish_passkey_registration", skip_all)]
    pub async fn finish_passkey_registration(
        &self,
        claims: &Claims,
        blob: &str,
        credential: &RegisterPublicKeyCredential,
        label: Option<String>,
    ) -> Result<PasskeySummary> {
        self.ensure_recent_login(claims)?;
        let (credential_id, passkey) = self.webauthn.finish_registration(claims.sub, blob, credential, now())?;
        let record =
            PasskeyRecord { user_id: claims.sub, credential_id, passkey, label, created_at: now(), last_used_at: None };
        let id = Uuid::now_v7();
        let summary = PasskeySummary::describing(id, &record);
        self.storage.create(id, record).await?;
        Ok(summary)
    }

    pub fn start_passkey_login(&self) -> Result<Ceremony> {
        self.webauthn.start_login(now())
    }

    #[tracing::instrument(name = "auth.finish_passkey_login", skip_all)]
    pub async fn finish_passkey_login(
        &self,
        blob: &str,
        credential: &PublicKeyCredential,
        device: Device,
    ) -> Result<Tokens> {
        let (user, credential_id) = self.webauthn.claimed_credential(credential)?;
        let mut found: Vec<(Uuid, PasskeyRecord)> = self.storage.matching(PASSKEY_CREDENTIAL, credential_id).await?;
        let (id, record) = found.pop().filter(|(_, record)| record.user_id == user).ok_or(Error::InvalidPasskey)?;
        let passkey = self.webauthn.finish_login(blob, credential, &record.passkey, now())?;
        let account = self.storage.find(user).await?.ok_or(Error::InvalidPasskey)?;
        self.storage.replace(id, PasskeyRecord { passkey, last_used_at: Some(now()), ..record }).await?;
        self.start_session(account, PASSKEY_METHOD, device).await
    }

    #[tracing::instrument(name = "auth.passkeys", skip_all)]
    pub async fn passkeys(&self, claims: &Claims) -> Result<Vec<PasskeySummary>> {
        let passkeys = self.user_passkeys(claims.sub).await?;
        Ok(passkeys.iter().map(|(id, record)| PasskeySummary::describing(*id, record)).collect())
    }

    #[tracing::instrument(name = "auth.remove_passkey", skip_all)]
    pub async fn remove_passkey(&self, claims: &Claims, id: Uuid) -> Result<()> {
        self.ensure_recent_login(claims)?;
        let mut found: Vec<(Uuid, PasskeyRecord)> = self.storage.matching(PASSKEY_ID, id.to_string()).await?;
        let (id, _) = found.pop().filter(|(_, record)| record.user_id == claims.sub).ok_or(Error::UnknownPasskey)?;
        Store::<PasskeyRecord>::remove(&*self.storage, id).await
    }

    async fn user_passkeys(&self, user: Uuid) -> Result<Vec<(Uuid, PasskeyRecord)>> {
        self.storage.matching(PASSKEY_USER, user.to_string()).await
    }

    fn ensure_recent_login(&self, claims: &Claims) -> Result<()> {
        let recent = now() <= claims.auth_time + self.settings.recent_auth_window.as_secs();
        recent.then_some(()).ok_or(Error::StepUpRequired)
    }

    #[tracing::instrument(name = "auth.start_password_reset", skip_all)]
    pub async fn start_password_reset(&self, email: &str, ip: Option<String>) -> Result<Uuid> {
        let email = normalized(email);
        self.admit_code_request(&email, ip.as_deref()).await?;
        let user_id = self.storage.find_by_email(&email).await?.map(|account| account.id);
        let reset = Subject { email, purpose: challenge::PASSWORD_RESET, user_id, ip, real: user_id.is_some() };
        self.open_challenge(&*self.storage, reset).await
    }

    async fn admit_code_request(&self, email: &str, ip: Option<&str>) -> Result<()> {
        self.code_sends.admit(ip, now())?;
        let limits = &self.settings.rate_limits;
        let sent: Vec<(Uuid, Challenge)> = self.storage.matching(CHALLENGE_EMAIL, email.to_owned()).await?;
        let window_start = now().saturating_sub(limits.code_sends_window.as_secs());
        let recent = sent.iter().filter(|(_, earlier)| earlier.created_at > window_start).count();
        let latest = sent.iter().map(|(_, earlier)| earlier.created_at).max();
        let spaced = latest.is_none_or(|latest| now() >= latest + limits.code_send_interval.as_secs());
        (spaced && recent < limits.code_sends_per_email).then_some(()).ok_or(Error::RateLimited)
    }

    async fn pending_verification(&self, user: Uuid) -> Result<Option<Uuid>> {
        let pending: Vec<(Uuid, Challenge)> = self.storage.matching(CHALLENGE_USER, user.to_string()).await?;
        let open = pending
            .into_iter()
            .filter(|(_, pending)| pending.purpose == challenge::VERIFY_EMAIL && pending.is_open(now()));
        Ok(open.max_by_key(|(_, pending)| pending.created_at).map(|(id, _)| id))
    }

    #[tracing::instrument(name = "auth.complete_password_reset", skip_all)]
    pub async fn complete_password_reset(
        &self,
        challenge_id: Uuid,
        code: &str,
        new_password: String,
        ip: Option<String>,
    ) -> Result<()> {
        self.code_checks.admit(ip.as_deref(), now())?;
        let password_hash = self.acceptable_hash(new_password).await?;
        let attempted = self.attempt(challenge_id).await?;
        let user = attempted.user_id.filter(|_| attempted.purpose == challenge::PASSWORD_RESET);
        let (Some(user), true) = (user, self.codes.matches(challenge_id, code, &attempted.code_hash)?) else {
            return Err(Error::InvalidCode);
        };
        let email = attempted.email.clone();
        let transaction = self.storage.begin().await?;
        transaction.replace(challenge_id, attempted.consumed(now())).await?;
        transaction.set_password(user, &password_hash).await?;
        transaction.mark_email_verified(user).await?;
        transaction.increment_security_version(user).await?;
        transaction.commit().await?;
        self.notify_in_background(notice::password_changed(email));
        Ok(())
    }

    async fn acceptable_hash(&self, password: String) -> Result<String> {
        password::check_policy(&password)?;
        self.breaches.check(&password).await?;
        password::hash(password).await
    }

    async fn upgrade_hash(&self, account: &Account, password: String) -> Result<()> {
        if account.password_hash.as_deref().is_some_and(password::is_outdated) {
            self.storage.set_password(account.id, &password::hash(password).await?).await?;
        }
        Ok(())
    }

    async fn attempt(&self, challenge_id: Uuid) -> Result<Challenge> {
        let mut found: Vec<(Uuid, Challenge)> = self.storage.matching(CHALLENGE_ID, challenge_id.to_string()).await?;
        let (_, pending) = found.pop().ok_or(Error::InvalidCode)?;
        if !pending.is_open(now()) {
            return Err(Error::ChallengeExpired);
        }
        let (previous_attempts, attempted) = (pending.attempts, pending.attempted());
        let counted = previous_attempts < self.settings.email_code_max_attempts
            && self
                .storage
                .replace_if(challenge_id, attempted.clone(), CHALLENGE_ATTEMPTS, Some(json!(previous_attempts)))
                .await?;
        if counted { Ok(attempted) } else { Err(Error::TooManyAttempts) }
    }

    async fn confirm(&self, challenge_id: Uuid, confirmed: Challenge) -> Result<Account> {
        let (email, user_id) = (confirmed.email.clone(), confirmed.user_id);
        let transaction = self.storage.begin().await?;
        transaction.replace(challenge_id, confirmed.consumed(now())).await?;
        let existing = match user_id {
            Some(id) => transaction.find(id).await?,
            None => transaction.find_by_email(&email).await?,
        };
        let account = if let Some(account) = existing {
            transaction.mark_email_verified(account.id).await?;
            Account { email_verified: true, ..account }
        } else {
            let account = new_account(email, true);
            transaction.register(&account).await?;
            account
        };
        transaction.commit().await?;
        Ok(account)
    }

    async fn open_challenge(&self, records: &dyn Records, subject: Subject) -> Result<Uuid> {
        let (challenge_id, code) = (Uuid::now_v7(), Codes::generate());
        let validity = self.settings.email_code_ttl;
        let pending = Challenge {
            email: subject.email.clone(),
            purpose: subject.purpose.into(),
            user_id: subject.user_id,
            code_hash: if subject.real { self.codes.hash(challenge_id, &code)? } else { String::new() },
            attempts: 0,
            expires_at: now() + validity.as_secs(),
            consumed_at: None,
            ip: subject.ip,
            created_at: now(),
        };
        records.create(challenge_id, pending).await?;
        if subject.real {
            self.notify_in_background(notice::verification_code(subject.email, &code, validity, subject.purpose));
        }
        Ok(challenge_id)
    }

    async fn verified_login(&self, account: Account, device: Device) -> Result<Tokens> {
        if account.email_verified {
            return self.start_session(account, PASSWORD_METHOD, device).await;
        }
        let admitted = self.admit_code_request(&account.email, device.ip.as_deref()).await;
        if let (Err(Error::RateLimited), Some(pending)) = (&admitted, self.pending_verification(account.id).await?) {
            return Err(Error::EmailUnverified(pending));
        }
        admitted?;
        let verification = Subject {
            email: account.email,
            purpose: challenge::VERIFY_EMAIL,
            user_id: Some(account.id),
            ip: device.ip,
            real: true,
        };
        Err(Error::EmailUnverified(self.open_challenge(&*self.storage, verification).await?))
    }

    fn notify_in_background(&self, notification: Notification) {
        tracing::info!(subject = notification.subject, "notifying an account owner");
        tokio::spawn(deliver(Arc::clone(&self.notifier), notification));
    }

    #[tracing::instrument(name = "auth.login", skip_all)]
    pub async fn login(&self, email: &str, password: String, device: Device) -> Result<Tokens> {
        let account = self.storage.find_by_email(&normalized(email)).await?;
        let hash = account.as_ref().and_then(|account| account.password_hash.clone());
        let known_password = hash.is_some();
        let password_matches = password::matches(password.clone(), hash).await?;
        match account {
            Some(account) if password_matches && known_password => {
                self.upgrade_hash(&account, password).await?;
                self.verified_login(account, device).await
            }
            _ => Err(Error::InvalidCredentials),
        }
    }

    #[tracing::instrument(name = "auth.refresh", skip_all)]
    pub async fn refresh(&self, refresh_token: &str) -> Result<Tokens> {
        let mut presented: Vec<(Uuid, RefreshToken)> =
            self.storage.matching(TOKEN_HASH, session::hash(refresh_token)).await?;
        let (token_id, token) = presented.pop().ok_or(Error::InvalidRefreshToken)?;
        let (session_id, session) = self.usable_session(token.session_id).await?;
        let account = self.storage.find(session.user_id).await?.ok_or(Error::InvalidRefreshToken)?;
        if let Some(reason) = self.revocation_reason(&account, &session, &token) {
            self.storage.replace(session_id, session.revoked(reason, now())).await?;
            return Err(Error::InvalidRefreshToken);
        }
        self.rotate(account, (session_id, session), (token_id, token)).await
    }

    #[tracing::instrument(name = "auth.logout", skip_all)]
    pub async fn logout(&self, claims: &Claims) -> Result<()> {
        self.revoke_session(claims, claims.sid).await.map_err(|_| Error::InvalidToken)
    }

    #[tracing::instrument(name = "auth.revoke_session", skip_all)]
    pub async fn revoke_session(&self, claims: &Claims, id: Uuid) -> Result<()> {
        let owned = self.usable_session(id).await.ok().filter(|(_, session)| session.user_id == claims.sub);
        let (session_id, session) = owned.ok_or(Error::UnknownSession)?;
        self.storage.replace(session_id, session.revoked("logout", now())).await
    }

    #[tracing::instrument(name = "auth.logout_everywhere", skip_all)]
    pub async fn logout_everywhere(&self, claims: &Claims) -> Result<()> {
        let transaction = self.storage.begin().await?;
        transaction.increment_security_version(claims.sub).await?;
        for (session_id, session) in usable_sessions(&*transaction, claims.sub).await? {
            transaction.replace(session_id, session.revoked("logout_all", now())).await?;
        }
        transaction.commit().await
    }

    #[tracing::instrument(name = "auth.sessions", skip_all)]
    pub async fn sessions(&self, claims: &Claims) -> Result<Vec<ActiveSession>> {
        let sessions = usable_sessions(&*self.storage, claims.sub).await?;
        Ok(sessions.into_iter().map(|(id, session)| ActiveSession::describing(id, session, claims.sid)).collect())
    }

    #[tracing::instrument(name = "auth.purge_ended_sessions", skip_all)]
    pub async fn purge_ended_sessions(&self) -> Result<u64> {
        let threshold = now().saturating_sub(self.settings.session_retention.as_secs());
        let mut purged = 0;
        for field in SESSION_END_FIELDS {
            purged += Store::<Session>::remove_before(&*self.storage, field, threshold).await?;
        }
        let ended_challenges = now().saturating_sub(CHALLENGE_RETENTION);
        Ok(purged + Store::<Challenge>::remove_before(&*self.storage, CHALLENGE_END, ended_challenges).await?)
    }

    pub fn start_cleanup(&self) {
        tokio::spawn(cleanup(self.clone()));
    }

    pub fn authenticate(&self, access_token: &str) -> Result<Claims> {
        self.keyring.verify(access_token)
    }

    fn revocation_reason(&self, account: &Account, session: &Session, token: &RefreshToken) -> Option<&'static str> {
        if account.security_version != session.security_version {
            Some("security_version_changed")
        } else if token.is_reused(now(), &self.settings) {
            Some("reuse_detected")
        } else {
            None
        }
    }

    async fn rotate(&self, account: Account, session: (Uuid, Session), token: (Uuid, RefreshToken)) -> Result<Tokens> {
        let ((session_id, session), (token_id, token)) = (session, token);
        let (successor, superseded) = (Uuid::now_v7(), token.replaced_by);
        let transaction = self.storage.begin().await?;
        let expected = superseded.map(|id| json!(id));
        if !transaction.replace_if(token_id, token.replaced(successor, now()), SUCCESSOR, expected).await? {
            return Err(Error::InvalidRefreshToken);
        }
        if let Some(superseded) = superseded {
            Store::<RefreshToken>::remove(&*transaction, superseded).await?;
        }
        let tokens = self.issue(&*transaction, account, (session_id, &session), successor).await?;
        transaction.replace(session_id, session.used(now(), &self.settings)).await?;
        transaction.commit().await?;
        Ok(tokens)
    }

    async fn start_session(&self, account: Account, method: &str, device: Device) -> Result<Tokens> {
        let session = Session::started(&account, method, device, now(), &self.settings);
        let session_id = Uuid::now_v7();
        let transaction = self.storage.begin().await?;
        transaction.create(session_id, session.clone()).await?;
        let tokens = self.issue(&*transaction, account, (session_id, &session), Uuid::now_v7()).await?;
        transaction.commit().await?;
        Ok(tokens)
    }

    async fn usable_session(&self, id: Uuid) -> Result<(Uuid, Session)> {
        let mut found: Vec<(Uuid, Session)> = self.storage.matching(SESSION_ID, id.to_string()).await?;
        found.pop().filter(|(_, session)| session.is_usable(now())).ok_or(Error::InvalidRefreshToken)
    }

    async fn issue(
        &self,
        records: &dyn Records,
        account: Account,
        (session_id, session): (Uuid, &Session),
        refresh_token_id: Uuid,
    ) -> Result<Tokens> {
        let (refresh_token, record) = RefreshToken::generated(session_id, now());
        records.create(refresh_token_id, record).await?;
        Ok(Tokens {
            access_token: self.keyring.sign(&account, session_id, session)?,
            token_type: "Bearer",
            expires_in: self.settings.access_token_ttl.as_secs(),
            refresh_token,
            refresh_expires_in: self.settings.refresh_idle_ttl.as_secs(),
            user: User { id: account.id, email: account.email },
        })
    }
}

async fn usable_sessions(records: &dyn Records, user: Uuid) -> Result<Vec<(Uuid, Session)>> {
    let mut sessions: Vec<(Uuid, Session)> = records.matching(SESSION_USER, user.to_string()).await?;
    sessions.retain(|(_, session)| session.is_usable(now()));
    Ok(sessions)
}

async fn cleanup(authenticator: Authenticator) {
    let mut interval = tokio::time::interval(authenticator.settings.session_cleanup_interval);
    loop {
        interval.tick().await;
        authenticator.purge_ended_sessions().await.map_or_else(|cause| log_failed_purge(&cause), log_purge);
    }
}

fn log_purge(purged: u64) {
    tracing::info!(purged, "ended sessions and email challenges purged");
}

fn log_failed_purge(cause: &Error) {
    tracing::error!(?cause, "ended sessions and email challenges could not be purged");
}

async fn deliver(notifier: Arc<dyn Notifier>, notification: Notification) {
    notifier.notify(&notification).await.ok();
}

struct Subject {
    email: String,
    purpose: &'static str,
    user_id: Option<Uuid>,
    ip: Option<String>,
    real: bool,
}

fn new_account(email: String, email_verified: bool) -> Account {
    Account {
        id: Uuid::now_v7(),
        email,
        display_name: None,
        role: DEFAULT_ROLE.into(),
        security_version: 0,
        email_verified,
        password_hash: None,
    }
}

fn normalized(email: &str) -> String {
    email.trim().to_lowercase()
}
