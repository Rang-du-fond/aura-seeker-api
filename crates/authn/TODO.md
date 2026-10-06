# authn — implementation steps

Tracks `authentication_spec.md`. Section numbers refer to the spec.

## MVP: password login

The signing key is an Ed25519 PKCS#8 PEM file: `openssl genpkey -algorithm ed25519 -out auth_signing_key.pem`.

- [x] 1. Crate skeleton in the workspace, `Error` with the spec's `{"error", "message"}` shape (§7)
- [x] 2. `Account` and the `Accounts` storage port
- [x] 3. Password policy (length only) and Argon2id hashing off the async runtime (§9.2)
- [x] 4. Access tokens: EdDSA JWT, `typ = at+jwt`, one signing key loaded from a PEM file (§4.1, §4.2)
- [x] 5. `Authenticator`: signup, login with a dummy hash for unknown emails, token verification (§6.3, §6.4)
- [x] 6. HTTP: `POST /auth/password/signup`, `POST /auth/password/login`, `GET /auth/me`, `AuthUser` extractor (§7)
- [x] 7. API side: `users` and `identities` migration, `Accounts` adapter, wiring in `main.rs` (§5)
- [x] 8. End-to-end check against SQLite

## MVP shortcuts to undo

- [x] Signup creates the account without verifying the email by code; the user then logs in (§6.3)
- [x] The code pepper falls back to a value derived from the signing key when `AUTH_CODE_PEPPER` is not set (§9.3)
- [x] Email codes are not rate limited per IP or per address yet (§9.1)
- [ ] Per-IP limits use the direct peer address: behind a reverse proxy every client shares the proxy's quota (§9.1)
- [ ] Per-IP limits are kept in memory, per instance, and reset on restart (§9.1)
- [ ] Passkey registration and removal need a login younger than 10 minutes; the step-up token alternative is not built (§6.8, §6.12)
- [ ] A passkey login blob can be replayed with its assertion for 5 minutes, since the server keeps no challenge state (§4.4)
- [ ] The AEAD key falls back to a value derived from the signing key when `AUTH_AEAD_KEY` is not set (§4.4)
- [ ] `passkeys.backup_eligible` is not stored as its own column (§5)
- [ ] A Google ID token can be replayed with its nonce until one of them expires, since the server keeps no nonce state (§6.6)
- [ ] Linking a provider records no `auth_event` (§6.7)
- [ ] Unlinking a sign-in method sends no notification and does not sign other devices out (§6.12, §9.6)
- [ ] The last-method rule counts identities and passkeys; it does not count email-code login as a remaining way in (§6.12)
- [x] Signup answers `409 email_taken`; the spec answers the same `202` either way (§9.4)
- [x] Signing up with an email already registered sends no "you already have an account" email (§6.3)
- [x] No `sid` claim, since there are no sessions yet (§4.1)
- [x] User and identity are inserted without a transaction
- [x] Token lifetime, issuer and audience come from constants and environment variables, not `[auth]` TOML (§11.1)
- [x] A refresh is several separate writes, not one transaction, so two concurrent refreshes of the same token can both succeed (§6.10)
- [x] Sessions do not record device label, user agent, IP or `security_version` (§5)
- [ ] Logout-all and password reset increment `security_version`; password change and MFA change are not built (§5)
- [ ] The session IP is the direct peer address, so behind a reverse proxy it is the proxy's (§5)
- [ ] Only the `[auth]` keys in use are read; the other keys of §11.1 are ignored until their features exist

## Next

- [x] Sessions and rotating refresh tokens with retry grace and reuse detection, `POST /auth/refresh`, `POST /auth/logout` (§4.3, §6.1, §6.10, §6.11)
- [x] `POST /auth/logout-all`, `GET /auth/sessions`, `DELETE /auth/sessions/{id}` (§6.11)
- [x] Cleanup job for expired or revoked sessions and their refresh tokens (§5)
- [x] Protect the API's resources with `AuthUser`: every resource route needs a token, except reading places, place statistics and tags, and downloading a file's content
- [x] `Mailer` trait with SMTP and log-only implementations (§11.2), now the separate `notifier` crate
- [x] Email challenges, email-code login and signup verification (§6.2, §6.3)
- [x] Password reset, HIBP check, rehash on login (§6.5, §9.2)
- [ ] Keyring with retired keys and `GET /.well-known/jwks.json` (§4.2)
- [ ] Rate limits for password login, MFA, refresh and the rest of `/auth` (§9.1)
- [ ] Audit events and notifications (§9.6)
- [x] Google native login: `GET /auth/nonce`, `POST /auth/google/token`, identity resolution and auto-linking (§6.6, §6.7)
- [x] Sign-in methods: `GET /auth/identities`, `POST /auth/google/link` (same verified email only), `DELETE /auth/identities/{id}` with the last-method rule (§6.12)
- [ ] Google redirect flows for web and system browser, `POST /auth/exchange`, and the `Provider` trait for other providers (§6.6, §10)
- [ ] MFA: TOTP, recovery codes (§6.9)
- [x] Passkeys: registration, discoverable login, list and removal (§6.8)
- [ ] Passkey as a second factor (§6.9)
- [ ] Step-up and account changes (§6.12)
- [ ] Web cookies and `X-Client` (§8)
