# jira-tui

A terminal UI for [Jira Cloud](https://developer.atlassian.com/cloud/jira/platform/rest/v3/intro/) built with [ratatui](https://ratatui.rs/). It authenticates with OAuth 2.0 (3LO), lists issues by sprint (plus the backlog), and supports search, sort, filter, and issue CRUD.

## Prerequisites

- Rust (stable)
- A Jira Cloud site

## Run

```bash
cargo run --release
```

On first launch, press Enter to log in. The TUI contacts the token service, opens Atlassian in the browser, and catches the redirect at `http://127.0.0.1:8787/callback`. `Ctrl+o` opens the link if the browser did not launch; copy the cyan URL from the login or waiting screen otherwise.

The TUI authenticates through a hosted token service that holds the Atlassian client secret, so you never handle credentials yourself. The default is `https://jira-tui-token-service-ptjqxekmlq-uc.a.run.app`; override it with `--token-service URL` or `JIRA_TUI_TOKEN_SERVICE` (the flag wins) if you have been given a different one to use.

If several sites are available, pick one; then pick a board. Tokens and preferences are stored under the XDG config directory:

- `~/.config/jira-tui/config.toml` — cloud id, board, columns, last sort/filter
- `~/.config/jira-tui/credentials.toml` — tokens (mode `0600`), used if the OS keyring is unavailable
- `~/.local/state/jira-tui/jira-tui.log` — application logs

Refresh tokens rotate. Each refresh overwrites the stored pair immediately.

Existing credentials from a user-owned OAuth app will not refresh through the hosted token service. Use `:logout` and log in once.

Until Atlassian finishes reviewing the shared app, you may see an unapproved-app warning when you first consent — that is expected. See the privacy policy: [PRIVACY.md](PRIVACY.md).

## Keybindings

| Key | Action |
| --- | --- |
| `j` / `k` | Move selection |
| `h` / `l` / `←` / `→` / `Tab` / `[` / `]` | Switch sprint / backlog tabs |
| `s` | Sort (persisted) |
| `f` | Filter (persisted) |
| `/` | Search current tab (session only) |
| `r` | Refresh |
| `p` | Change project / board |
| `Enter` | View issue |
| `n` / `c` | Create story |
| `e` | Edit issue |
| `d` | Delete issue (confirm) |
| `a` | Assign (`Ctrl+u` unassigns) |
| `t` | Transition |
| `:` | Command (`logout`, `login`, `project`, `quit`) |
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
columns = ["key", "summary", "issuetype", "priority", "status", "assignee", "story_points"]

[view]
sort_field = "priority"
sort_dir = "desc"
filter_statuses = []
filter_types = []
filter_assignee = { accounts = [], unassigned = false }
```

An empty `filter_assignee` matches every assignee. Older configs (`"any"`, `"me"`, `"unassigned"`, `{ account = "..." }`) still load. `"me"` is stored as your account id once login can see it, so it is not kept alongside that id.

The story points field is discovered from the board estimation configuration when possible (classic “Story Points” vs team-managed “Story point estimate”).

## API facades

- **Search** — `POST /rest/api/3/search/jql` via `SearchBuilder` (project, sprint/backlog, text, status, type, assignee, `ORDER BY`, token pagination)
- **Issues** — get / create / update / delete / assign / transition
- **Agile** — boards, active+future sprints, board configuration

Calls go to `https://api.atlassian.com/ex/jira/{cloudId}/...` with a Bearer access token. Token exchange and refresh go through the token service.

## Tests

```bash
cargo test
```
