# Privacy policy

Last updated: 21 September 2026.

jira-tui and token-service do not collect data for this project. There is no account, no telemetry, and no server operated by the project. Your Jira data stays between the computer where you run jira-tui and Atlassian.

## On your computer

jira-tui stores:

- OAuth access and refresh tokens in the OS keyring and in `~/.config/jira-tui/credentials.toml` (mode `0600`)
- The OAuth client id, selected site, board, and view preferences in `~/.config/jira-tui/config.toml`
- Operational logs in `~/.local/state/jira-tui/`

Logs record failures and counts. They are not a copy of your issues or tokens.

`:logout` revokes the tokens at Atlassian and deletes the local copies. Preferences stay until you delete the config file. Uninstalling is deleting those files and the keyring entry named `jira-tui` / `oauth-tokens`.

## What leaves your computer

jira-tui calls your Jira Cloud site at `api.atlassian.com` with your access token. You consent to `read:jira-work`, `write:jira-work`, and `offline_access`. Data stored in Jira is covered by Atlassian’s privacy policy.

Login opens Atlassian in your browser and returns to `http://127.0.0.1:8787/callback` on your machine.

token-service holds the OAuth client secret in memory and uses it only to exchange an authorization code, refresh a token, or revoke a token at `auth.atlassian.com`. It does not write the secret, codes, or tokens to disk. It never receives issue content.

It listens on `127.0.0.1` unless `BIND_ADDR` or `PORT` says otherwise. If `JIRA_TUI_TOKEN_SERVICE` points at another host, that host receives the authorization code, PKCE verifier, and any token you refresh or revoke, for that request only.

## Shared token-service

Whoever runs token-service for other people is the operator of that process. Keep the client secret in the environment, do not log request bodies, and discard codes and tokens when Atlassian’s response has been returned.

## Changes

A change in what these programs store or send will be made in this file in the same commit.
