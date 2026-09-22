use ratatui::text::Line;

pub const HELP_LINES: &[&str] = &[
    "Navigation",
    "  j / k / Down / Up     move selection",
    "  PageUp / PageDown     jump",
    "  g / G                 top / bottom",
    "  h / l / ← / →         previous / next sprint or backlog",
    "  Tab / Shift+Tab       next / previous sprint or backlog",
    "  [ / ]                 previous / next sprint or backlog",
    "",
    "Board",
    "  s                     sort (Enter apply, d toggle direction)",
    "  f                     filter (persisted)",
    "  /                     search current tab (session only)",
    "  r                     refresh",
    "  p                     change project / board",
    "  Enter                 view issue",
    "  n / c                 create story",
    "  e                     edit issue",
    "  d                     delete issue",
    "  a                     assign",
    "  t                     transition",
    "",
    "Login",
    "  Enter                 log in with Atlassian (opens the browser)",
    "  Ctrl+o                open the generated Atlassian login link",
    "",
    "Commands",
    "  :logout               revoke tokens and return to login",
    "  :login                re-run authorization",
    "  :project              change project / board",
    "  :quit / :q            quit",
    "  ?                     toggle this help",
    "  Esc                   close overlay / go back",
    "  q                     quit (stays logged in)",
];

pub fn help_lines() -> Vec<Line<'static>> {
    HELP_LINES.iter().copied().map(Line::from).collect()
}
