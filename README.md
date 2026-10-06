# Aura Seeker API

REST API written in Rust with [axum](https://github.com/tokio-rs/axum). It stores data in SQLite, uploaded files on the local filesystem, and answers in `application/hal+json`.

## Requirements

- A recent stable Rust toolchain with `cargo` (developed with Rust 1.99)
- `openssl`, to generate the authentication key

## 1. Generate the authentication key

Access tokens are signed with an Ed25519 key. The server refuses to start without it.

```sh
openssl genpkey -algorithm ed25519 -out auth_signing_key.pem
```

Run this once, in this directory. The file is git-ignored: keep it secret, and use the same file on every instance. Replacing it invalidates every access token already issued.

## 2. Generate the code secret

Email codes are stored hashed with a secret the server reads from `AUTH_CODE_PEPPER`. It refuses to start without it.

```sh
export AUTH_CODE_PEPPER="$(openssl rand -base64 32)"
```

Generate it once and reuse the same value at every start and on every instance; keep it out of git.

## 3. Compile

```sh
cargo build --release
```

The binary is `target/release/aura-api`.

## 4. Run

```sh
cargo run --release
```

On first start the server creates `aura.db`, applies the SQL files from `migrations/` and listens on port 8080. Uploaded files go to `uploads/`.

Check that it works:

```sh
curl http://localhost:8080/
```

This entry point is public; the resources it links to need a token (see "Logging in").

`GET /health` answers `200 {"status": "ok"}` when the server is up and can reach its database, and `503` otherwise.

## Configuration

The server reads environment variables and one settings file; all are optional.

| Variable | Default | Purpose |
| --- | --- | --- |
| `BIND_ADDRESS` | `0.0.0.0:8080` | Address and port to listen on |
| `DATABASE_URL` | `sqlite://aura.db?mode=rwc` | Database connection |
| `STORAGE_URL` | `file://<current directory>/uploads` | Where uploaded files are stored |
| `CONFIG_FILE` | `config.toml` | Authentication settings file, see below |
| `AUTH_AEAD_KEY` | derived from the signing key | Secret that encrypts passkey ceremony state |
| `AUTH_CODE_PEPPER` | none, **required** | Secret of at least 32 characters mixed into the stored hash of email codes |
| `SMTP_URL` | unset | SMTP server used to send emails; unset means emails are only logged |
| `RUST_LOG` | `info,sqlx=warn` | Log filter; e.g. `warn` silences request logs, `info,sqlx=info` adds every SQL statement |
| `LOG_FORMAT` | text | `json` writes one JSON object per log line |
| `OTEL_TRACES_EXPORTER` | none | `otlp` exports traces over gRPC, `console` prints them to standard output |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | Collector that receives the traces when the exporter is `otlp` |
| `OTEL_SERVICE_NAME` | `aura-api` | Service name attached to exported traces |

Example:

```sh
BIND_ADDRESS=127.0.0.1:3000 RUST_LOG=warn cargo run --release
```

Every request is logged on one line with its method and route, path, status, duration, a request id and, when authenticated, the user id. The request id is also returned in the `x-request-id` response header. Responses with a 5xx status are logged as errors.

Requests and the operations behind them (services, SQL, file storage, authentication, notifications) are traced. With `OTEL_TRACES_EXPORTER=otlp`, those traces are sent to an OpenTelemetry collector, and an incoming `traceparent` header continues the caller's trace:

```sh
OTEL_TRACES_EXPORTER=otlp OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 cargo run --release
```

Authentication is configured in the `[auth]` section of `config.toml`, read from the directory the server is started in. The file in this repository lists every setting with its default value: issuer, audience, token lifetimes (`"15m"`, `"90d"`…) and the path of the signing key. A missing file or a missing setting falls back to those defaults.

To send emails, set `SMTP_URL` to an SMTP URL such as `smtps://user:password@smtp.example.com:465`, and the sender address in the `[notifications.email]` section of `config.toml`. Without it, emails are only written to the log.

## Logging in

Every endpoint needs an access token, except signup, login and refresh, the entry point `/`, `/health`, `/openapi.json`, reading places and tags (`GET /places`, `GET /places/{id}`, `GET /places/statistics`, `GET /tags`, `GET /tags/{id}`) and downloading a file's content.

Create an account, confirm the 6-digit code sent to its email address, then use the returned `access_token` as a bearer token. Signing up answers `202` with a `challenge_id` whether or not the email was already registered. Passwords must be 12 to 128 characters long. Without `SMTP_URL`, the code is written to the server log instead of being emailed. An access token is valid for 15 minutes; exchange the `refresh_token` for a new pair before it expires. Each refresh token works once, and a device stays signed in as long as it refreshes at least every 90 days.

```sh
curl -X POST http://localhost:8080/auth/password/signup \
  -H 'content-type: application/json' \
  -d '{"email": "me@example.com", "password": "correct horse battery"}'

curl -X POST http://localhost:8080/auth/email/verify \
  -H 'content-type: application/json' \
  -d '{"challenge_id": "<challenge_id>", "code": "<code from the email>"}'

curl -X POST http://localhost:8080/auth/password/login \
  -H 'content-type: application/json' \
  -d '{"email": "me@example.com", "password": "correct horse battery"}'

curl http://localhost:8080/auth/me -H 'authorization: Bearer <access_token>'

curl -X POST http://localhost:8080/auth/refresh \
  -H 'content-type: application/json' \
  -d '{"refresh_token": "<refresh_token>"}'

curl -X POST http://localhost:8080/auth/logout -H 'authorization: Bearer <access_token>'
```

A code is valid for 10 minutes and 5 attempts. Logging in with a password before the address is confirmed answers `403 email_unverified` with a new `challenge_id`, and emails a new code. `POST /auth/email/start` with `{"email": …}` emails a code to log in without a password; verifying it creates the account if the address is new.

To reset a forgotten password, `POST /auth/password/reset/start` with `{"email": …}` emails a code, and `POST /auth/password/reset/complete` with `{challenge_id, code, new_password}` sets the new password and signs out every device. The user then logs in with it.

A new password is refused if it appears in the Have I Been Pwned breach list; only the first five characters of its SHA-1 hash leave the server. If that service cannot be reached, the password is accepted and a warning is logged. `check_breached_passwords = false` in `config.toml` turns the check off.

A logged-in user can add a passkey with `POST /auth/passkeys/register/start` then `/finish`, and later log in without typing anything with `POST /auth/passkeys/login/start` then `/finish`. Adding or removing a passkey needs a login less than 10 minutes old. For passkeys to work, set the `[auth.webauthn]` section of `config.toml` to your real domain: `rp_id` and the `origins` the app runs on.

Requests that send or check a code are rate limited, and answer `429 rate_limited` beyond: 10 codes an hour per IP address, 5 an hour and one a minute per email address, and 30 code checks per 15 minutes per IP address. The limits are in the `[auth.rate_limits]` section of `config.toml`.

To log in with Google from the Android app, get a nonce from `GET /auth/nonce`, pass it to the Google sign-in dialog, then send the resulting ID token to `POST /auth/google/token` with `{id_token, nonce}`. The Google client IDs go in the `[auth.providers.google]` section of `config.toml`. A first Google login creates the account; if an account already uses that email address, Google is added to it.

`GET /auth/identities` lists the account's sign-in methods (password, Google). `POST /auth/google/link` with `{id_token, nonce}` attaches the Google account that has the same verified email address as the signed-in user, and `DELETE /auth/identities/{id}` removes a method unless it is the last one. Linking and removing need a login less than 10 minutes old.

Emails are never sent to addresses on domains reserved for tests, such as `example.com`.

`GET /auth/sessions` lists the devices you are signed in on, `DELETE /auth/sessions/{id}` signs one out and `POST /auth/logout-all` signs out all of them. Sessions that ended more than 30 days ago are deleted by an hourly job.

```sh
curl http://localhost:8080/auth/sessions -H 'authorization: Bearer <access_token>'
```

## Fake data

With the server running, create realistic fake places in Auvergne-Rhône-Alpes cities (Python 3, no dependency). Run it from the directory the server is started in, or pass `--database`: it marks its fake authors as verified directly in the SQLite file.

```sh
python3 scripts/seed_places.py 200
```

`--url` targets another server, `--seed` makes the places reproducible.

To start again from an empty database, stop the server and run, from the directory it is started in:

```sh
python3 scripts/wipe_database.py
```

It deletes the SQLite database and the uploaded files after asking for confirmation (`--yes` skips the question).

## API documentation

`openapi.json` describes every endpoint (OpenAPI 3.1). The running server serves the same document, so regenerate the file after changing an endpoint:

```sh
curl http://localhost:8080/openapi.json > openapi.json
```

## Adding a migration

Add `migrations/<next number>_<name>.sql` and rebuild. Every `.sql` file of that directory is embedded in the binary and applied in file-name order at the next start, then recorded in the `migrations` table.
