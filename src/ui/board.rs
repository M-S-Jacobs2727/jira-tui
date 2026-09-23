use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::app::App;
use crate::jira::models::Issue;
use crate::ui::Fill;

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    frame.render_widget(Fill, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);

    draw_tabs(frame, app, chunks[0]);
    draw_table(frame, app, chunks[1]);
    draw_footer(frame, app, chunks[2]);
}

fn draw_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let board = app.config.project_key().unwrap_or("Jira");
    let title = if app.tabs.is_empty() {
        board.to_string()
    } else {
        format!("{board}  {}/{}", app.tab_idx + 1, app.tabs.len())
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 {
        return;
    }
    let labels: Vec<String> = if app.tabs.is_empty() {
        vec!["loading…".into()]
    } else {
        app.tabs.iter().map(|tab| tab.title.clone()).collect()
    };
    let selected = if app.tabs.is_empty() {
        0
    } else {
        app.tab_idx.min(app.tabs.len().saturating_sub(1))
    };
    frame.render_widget(
        Paragraph::new(tab_strip_line(&labels, selected, inner.width as usize)),
        inner,
    );
}

fn tab_strip_line(labels: &[String], selected: usize, width: usize) -> Line<'static> {
    if labels.is_empty() || width == 0 {
        return Line::default();
    }
    let selected = selected.min(labels.len() - 1);
    let mut start = selected;
    let mut end = selected + 1;
    let mut used = labels[selected].chars().count();

    let marks =
        |start: usize, end: usize| usize::from(start > 0) * 2 + usize::from(end < labels.len()) * 2;

    loop {
        if end < labels.len() {
            let extra = 3 + labels[end].chars().count();
            if used + extra + marks(start, end + 1) <= width {
                used += extra;
                end += 1;
                continue;
            }
        }
        if start > 0 {
            let extra = 3 + labels[start - 1].chars().count();
            if used + extra + marks(start - 1, end) <= width {
                used += extra;
                start -= 1;
                continue;
            }
        }
        break;
    }

    let selected_budget = width.saturating_sub(marks(start, end));
    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::raw("‹ "));
    }
    for (i, label) in labels[start..end].iter().enumerate() {
        let idx = start + i;
        if i > 0 {
            spans.push(Span::raw(" │ "));
        }
        let text = if idx == selected && end - start == 1 && label.chars().count() > selected_budget
        {
            truncate_label(label, selected_budget)
        } else {
            label.clone()
        };
        if idx == selected {
            spans.push(Span::styled(
                text,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::raw(text));
        }
    }
    if end < labels.len() {
        spans.push(Span::raw(" ›"));
    }
    Line::from(spans)
}

fn truncate_label(label: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = label.chars().count();
    if count <= max {
        return label.to_string();
    }
    if max == 1 {
        return "…".into();
    }
    let keep: String = label.chars().take(max - 1).collect();
    format!("{keep}…")
}

fn draw_table(frame: &mut Frame, app: &mut App, area: Rect) {
    let columns = app.config.columns();
    let widths = column_widths(&columns, area.width);
    // Table lays out columns in the area left after borders and the "> " highlight.
    let cell_area = area.width.saturating_sub(4);
    let resolved = resolve_column_widths(&columns, cell_area);
    let lead_summary = summary_leads_to_type(&columns);

    let header = Row::new(columns.iter().map(|col| {
        Cell::from(column_label(col)).style(Style::default().add_modifier(Modifier::BOLD))
    }))
    .height(1);

    let story_points_field = app.config.story_points_field_cloned();
    let selected = app.current_tab().map(|t| t.selected).unwrap_or(0);
    let issue_count = app.current_tab().map(|t| t.issues.len()).unwrap_or(0);
    let has_next = app
        .current_tab()
        .and_then(|t| t.next_page.as_ref())
        .is_some();

    let mut rows: Vec<Row> = app
        .current_tab()
        .map(|tab| {
            tab.issues
                .iter()
                .map(|issue| {
                    Row::new(
                        columns
                            .iter()
                            .enumerate()
                            .map(|(i, col)| {
                                let mut value =
                                    cell_value(issue, col, story_points_field.as_deref());
                                if lead_summary && col == "summary" {
                                    let width = resolved.get(i).copied().unwrap_or(20) as usize;
                                    value = with_leaders(&value, width);
                                }
                                Cell::from(value)
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let inner_rows = area.height.saturating_sub(4) as usize;
    app.list_rows = inner_rows;
    while rows.len() < inner_rows {
        rows.push(Row::new(
            columns.iter().map(|_| Cell::from(" ")).collect::<Vec<_>>(),
        ));
    }

    let offset = visible_offset(selected, inner_rows, issue_count, app.list_offset);
    let more_up = offset > 0;
    let more_down = offset.saturating_add(inner_rows) < issue_count || has_next;
    let title = match (more_up, more_down) {
        (true, true) => "issues ↑↓",
        (true, false) => "issues ↑",
        (false, true) => "issues ↓",
        (false, false) => "issues",
    };

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = TableState::default()
        .with_selected(Some(selected))
        .with_offset(offset);
    frame.render_stateful_widget(table, area, &mut state);
    app.list_offset = state.offset();
}

fn visible_offset(selected: usize, visible: usize, len: usize, current: usize) -> usize {
    if visible == 0 || len == 0 {
        return 0;
    }
    let mut offset = current.min(len.saturating_sub(1));
    if selected < offset {
        offset = selected;
    } else if selected >= offset.saturating_add(visible) {
        offset = selected + 1 - visible;
    }
    offset.min(len.saturating_sub(visible.min(len)))
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let view = app.current_view().cloned().unwrap_or_default();
    let assignee = view.filter_assignee.footer_label(
        app.self_account_id.as_deref(),
        |id| app.user_display_name(id),
    );
    let filter = format!(
        "status=[{}] type=[{}] assignee={assignee}",
        view.filter_statuses.join(","),
        view.filter_types.join(",")
    );
    let search = if app.search_query.is_empty() {
        String::new()
    } else {
        format!("  search={}", app.search_query)
    };
    let sort = if !view.sort_field.has_direction() {
        format!("sort={}", view.sort_field.label().to_ascii_lowercase())
    } else {
        format!("sort={} {}", view.sort_field.label(), view.sort_dir.label())
    };
    let text = format!("{sort}  {filter}{search}   s sort  f filter  / search  ? help");
    frame.render_widget(
        Paragraph::new(Span::raw(text)).block(Block::default().borders(Borders::TOP)),
        area,
    );
}

fn column_label(col: &str) -> &'static str {
    match col {
        "key" => "Key",
        "issuetype" | "type" => "Type",
        "priority" => "Priority",
        "status" => "Status",
        "assignee" => "Assignee",
        "story_points" => "Points",
        "summary" => "Summary",
        _ => "Field",
    }
}

fn column_widths(columns: &[String], total: u16) -> Vec<Constraint> {
    columns
        .iter()
        .map(|col| match col.as_str() {
            "key" => Constraint::Length(10),
            "issuetype" | "type" => Constraint::Length(10),
            "priority" => Constraint::Length(10),
            "status" => Constraint::Length(14),
            "assignee" => Constraint::Length(16),
            "story_points" => Constraint::Length(7),
            "summary" => Constraint::Min(20),
            _ => Constraint::Length(12),
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|c| match c {
            Constraint::Length(n) if total < 40 => Constraint::Length(n.min(8)),
            other => other,
        })
        .collect()
}

fn resolve_column_widths(columns: &[String], available: u16) -> Vec<u16> {
    let constraints = column_widths(columns, available);
    let spacing = columns.len().saturating_sub(1) as u16;
    let mut fixed = 0u16;
    let mut flex = 0u16;
    for c in &constraints {
        match c {
            Constraint::Length(n) => fixed = fixed.saturating_add(*n),
            Constraint::Min(_) => flex = flex.saturating_add(1),
            _ => fixed = fixed.saturating_add(12),
        }
    }
    let rest = available.saturating_sub(fixed).saturating_sub(spacing);
    let each = if flex > 0 { rest / flex } else { 0 };
    constraints
        .into_iter()
        .map(|c| match c {
            Constraint::Length(n) => n,
            Constraint::Min(n) => each.max(n),
            _ => 12,
        })
        .collect()
}

fn summary_leads_to_type(columns: &[String]) -> bool {
    columns.windows(2).any(|pair| {
        pair[0] == "summary" && matches!(pair[1].as_str(), "issuetype" | "type")
    })
}

fn with_leaders(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() >= width {
        return chars.into_iter().take(width).collect();
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
    while out.chars().count() < width {
        out.push('.');
    }
    out
}

fn cell_value(issue: &Issue, col: &str, _story_points_field: Option<&str>) -> String {
    match col {
        "key" => issue.key.clone(),
        "issuetype" | "type" => issue.issue_type.clone(),
        "priority" => issue.priority.clone().unwrap_or_else(|| "-".into()),
        "status" => issue.status.clone(),
        "assignee" => issue
            .assignee
            .as_ref()
            .map(|u| u.display_name.clone())
            .unwrap_or_else(|| "Unassigned".into()),
        "story_points" => issue
            .story_points
            .map(|n| {
                if n.fract() == 0.0 {
                    format!("{n:.0}")
                } else {
                    format!("{n}")
                }
            })
            .unwrap_or_else(|| "-".into()),
        "summary" => issue.summary.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_strip_keeps_selected_visible() {
        let labels = ["Alpha", "Backlog"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let line = tab_strip_line(&labels, 1, 10);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("Backlog"));
        assert!(text.contains('‹'));
    }

    #[test]
    fn tab_strip_truncates_oversized_label() {
        let labels = vec!["this-name-is-very-long".into()];
        let line = tab_strip_line(&labels, 0, 8);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with('…'));
        assert!(text.chars().count() <= 8);
    }

    #[test]
    fn leaders_fill_to_width() {
        let padded = with_leaders("Hello", 12);
        assert_eq!(padded, "Hello ......");
        assert_eq!(padded.chars().count(), 12);
    }

    #[test]
    fn summary_type_adjacency() {
        let cols = ["key", "summary", "issuetype", "status"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(summary_leads_to_type(&cols));
        let reordered = ["key", "issuetype", "summary"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        assert!(!summary_leads_to_type(&reordered));
    }
}
