# token-service

Holds the Atlassian OAuth client secret so `jira-tui` can log in without shipping credentials in the binary.

The TUI still catches the browser redirect at `http://127.0.0.1:8787/callback`. This service only exchanges the authorization code, refreshes tokens, and revokes them.

Point `jira-tui` at a non-default bind address with `JIRA_TUI_TOKEN_SERVICE`.

## Run

```bash
export ATLASSIAN_CLIENT_ID=...
export ATLASSIAN_CLIENT_SECRET=...
cargo run -p token-service
```

Listens on `http://127.0.0.1:8788` by default.

| Variable | Default |
| --- | --- |
| `ATLASSIAN_CLIENT_ID` | required |
| `ATLASSIAN_CLIENT_SECRET` | required |
| `OAUTH_REDIRECT_URI` | `http://127.0.0.1:8787/callback` |
| `BIND_ADDR` | `127.0.0.1:8788` |

The redirect URI must match the TUI callback and the Atlassian app setting character-for-character.

## Endpoints

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/health` | liveness |
| `GET` | `/v1/config` | `client_id` and `redirect_uri` for the TUI |
| `POST` | `/v1/oauth/exchange` | `{ "code", "code_verifier" }` |
| `POST` | `/v1/oauth/refresh` | `{ "refresh_token" }` |
| `POST` | `/v1/oauth/revoke` | `{ "token" }` |

## Atlassian app

1. Create an OAuth 2.0 (3LO) app in the [developer console](https://developer.atlassian.com/console/myapps/).
2. Set the callback URL to `http://127.0.0.1:8787/callback`.
3. Enable classic scopes `read:jira-work` and `write:jira-work`.
4. Under **Distribution**, enable sharing (privacy policy URL required) so other users can consent.
