# jira-tui

A terminal UI for [Jira Cloud](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/) built with [ratatui](https://ratatui.rs/). It authenticates with OAuth 2.0 (3LO), lists issues by sprint (plus the backlog), and supports search, sort, filter, and issue CRUD.

## Prerequisites

- Rust (stable)
- A Jira Cloud site
- The in-repo [token-service](token-service/README.md) running (it holds the Atlassian client secret)

## Run

Start the token broker (once per machine / session):

```bash
export ATLASSIAN_CLIENT_ID=...
export ATLASSIAN_CLIENT_SECRET=...
cargo run -p token-service
```

Then in another terminal:

```bash
cargo run --release
```

On first launch, press Enter to log in. The TUI contacts the token service, opens Atlassian in the browser, and catches the redirect at `http://127.0.0.1:8787/callback`. `Ctrl+o` opens the link if the browser did not launch; copy the cyan URL from the login or waiting screen otherwise.

Point the TUI at a non-default broker with `JIRA_TUI_TOKEN_SERVICE` (default `http://127.0.0.1:8788`).

If several sites are available, pick one; then pick a board. Tokens and preferences are stored under the XDG config directory:

- `~/.config/jira-tui/config.toml` — cloud id, board, columns, last sort/filter
- `~/.config/jira-tui/credentials.toml` — tokens (mode `0600`), used if the OS keyring is unavailable
- `~/.local/state/jira-tui/jira-tui.log` — application logs

Refresh tokens rotate. Each refresh overwrites the stored pair immediately.

Existing credentials from a user-owned OAuth app will not refresh through the broker. Use `:logout` and log in once.

## Atlassian app (operators)

Create **one** shared OAuth 2.0 (3LO) app in the [developer console](https://developer.atlassian.com/console/myapps/). End users do not create their own app. Details: [token-service/README.md](token-service/README.md).

Callback URL (exact string):

`http://127.0.0.1:8787/callback`

Classic scopes (must match the authorize URL or Atlassian returns `401` / “scope does not match”):

| Scope | Why |
| --- | --- |
| `read:jira-work` | Search and read issues |
| `write:jira-work` | Create, edit, delete, assign, transition |

`offline_access` is requested automatically (refresh tokens). You do not add it in the console. After changing scopes, use `:logout` and log in again so Atlassian re-prompts for consent.

Enable **Distribution → sharing** so other users can consent. Until Atlassian reviews the app, they may see an unapproved-app warning.

See [OAuth 2.0 (3LO) apps](https://developer.atlassian.com/cloud/jira/platform/oauth-2-3lo-apps/) and [Other integrations](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/#other-integrations).

## Keybindings

| Key | Action |
| --- | --- |
| `j` / `k` | Move selection |
| `h` / `l` / `←` / `→` / `Tab` / `[` / `]` | Switch sprint / backlog tabs |
| `s` | Sort (persisted) |
| `f` | Filter (persisted) |
| `/` | Search current tab (session only) |
| `r` | Refresh |
| `Enter` | View issue |
| `n` / `c` | Create story |
| `e` | Edit issue |
| `d` | Delete issue (confirm) |
| `a` | Assign (`Ctrl+u` unassigns) |
| `t` | Transition |
| `:` | Command (`logout`, `login`, `quit`) |
| `?` | Help |
| `Esc` | Close overlay |
| `q` | Quit (stays logged in) |

`:logout` revokes the access/refresh tokens and returns to the login screen. Board and view preferences are kept.

## Config

`~/.config/jira-tui/config.toml`:

```toml
client_id = "..."          # filled in after login (shared app id)
cloud_id = "..."
board_id = 123
project_key = "ABC"
story_points_field = "customfield_10016"
columns = ["key", "issuetype", "priority", "status", "assignee", "story_points", "summary"]

[view]
sort_field = "priority"
sort_dir = "desc"
filter_statuses = []
filter_types = []
filter_assignee = "any"   # any | me | unassigned | { account = "..." }
```

The story points field is discovered from the board estimation configuration when possible (classic “Story Points” vs team-managed “Story point estimate”).

## API facades

- **Search** — `POST /rest/api/3/search/jql` via `SearchBuilder` (project, sprint/backlog, text, status, type, assignee, `ORDER BY`, token pagination)
- **Issues** — get / create / update / delete / assign / transition
- **Agile** — boards, active+future sprints, board configuration

Calls go to `https://api.atlassian.com/ex/jira/{cloudId}/...` with a Bearer access token. Token exchange and refresh go through the token service.

## Tests

```bash
cargo test --workspace
```
