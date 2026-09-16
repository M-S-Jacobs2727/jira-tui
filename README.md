# jira-tui

A terminal UI for [Jira Cloud](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/) built with [ratatui](https://ratatui.rs/). It authenticates with OAuth 2.0 (3LO), lists issues by sprint (plus the backlog), and supports search, sort, filter, and issue CRUD.

## Prerequisites

- Rust (stable)
- A Jira Cloud site
- An OAuth 2.0 (3LO) app in the [Atlassian developer console](https://developer.atlassian.com/console/myapps/)

## Create the OAuth app

1. Open the developer console and create an app (or select an existing one).
2. Under **Authorization**, configure **OAuth 2.0 (3LO)**.
3. Set the callback URL to exactly:

   `http://127.0.0.1:8787/callback`

   Atlassian matches this string character-for-character. The TUI always binds that host and port.
4. Under **Permissions**, add **Jira Cloud platform API**.
5. Click **Configure** (or **Granular scopes** / **Classic scopes**) and enable these **classic** scopes. They must match the authorize URL exactly or Atlassian returns `401` / “scope does not match”:

   | Scope | Why |
   | --- | --- |
   | `read:jira-work` | Search and read issues |
   | `write:jira-work` | Create, edit, delete, assign, transition |

   `offline_access` is requested automatically (refresh tokens). You do not add it in the console.

   Adding the API is not enough — each classic scope has to be enabled. After changing scopes, use `:logout` and log in again so Atlassian re-prompts for consent.

6. Copy the **Client ID** and **Secret** from the app settings.

See [OAuth 2.0 (3LO) apps](https://developer.atlassian.com/cloud/jira/platform/oauth-2-3lo-apps/) and [Other integrations](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/#other-integrations).

## Run

```bash
cargo run --release
```

On first launch, enter the client id and secret. The login screen generates an Atlassian authorize URL as soon as a client id is present. Press Enter to start the local callback listener and open that link, or `Ctrl+o` to open it in the browser. If the browser does not launch, copy the cyan URL from the login or waiting screen. If several sites are available, pick one; then pick a board. Tokens and preferences are stored under the XDG config directory:

- `~/.config/jira-tui/config.toml` — client id, cloud id, board, columns, last sort/filter
- `~/.config/jira-tui/credentials.toml` — tokens (mode `0600`), used if the OS keyring is unavailable
- `~/.local/state/jira-tui/jira-tui.log` — application logs

Refresh tokens rotate. Each refresh overwrites the stored pair immediately.

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
client_id = "..."
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

Calls go to `https://api.atlassian.com/ex/jira/{cloudId}/...` with a Bearer access token.

## Tests

```bash
cargo test
```
