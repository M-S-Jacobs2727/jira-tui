use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::app::App;
use crate::jira::models::Issue;
use crate::jira::search::AssigneeFilter;
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
    let board = app.config.project_key.as_deref().unwrap_or("Jira");
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
    let columns = &app.config.columns;
    let widths = column_widths(columns, area.width);
    let header = Row::new(columns.iter().map(|col| {
        Cell::from(column_label(col)).style(Style::default().add_modifier(Modifier::BOLD))
    }))
    .height(1);

    let story_points_field = app.config.story_points_field.clone();
    let selected = app.current_tab().map(|t| t.selected).unwrap_or(0);
    let mut rows: Vec<Row> = app
        .current_tab()
        .map(|tab| {
            tab.issues
                .iter()
                .map(|issue| {
                    Row::new(
                        columns
                            .iter()
                            .map(|col| {
                                Cell::from(cell_value(issue, col, story_points_field.as_deref()))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let inner_rows = area.height.saturating_sub(4) as usize;
    while rows.len() < inner_rows {
        rows.push(Row::new(
            columns.iter().map(|_| Cell::from(" ")).collect::<Vec<_>>(),
        ));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("issues"))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");

    let mut state = TableState::default().with_selected(Some(selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let view = &app.config.view;
    let assignee = match &view.filter_assignee {
        AssigneeFilter::Any => "any".to_string(),
        AssigneeFilter::Me => "me".to_string(),
        AssigneeFilter::Unassigned => "unassigned".to_string(),
        AssigneeFilter::Account(id) => id.clone(),
    };
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
    let text = format!(
        "sort={} {}  {filter}{search}   s sort  f filter  / search  ? help",
        view.sort_field.label(),
        view.sort_dir.label()
    );
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
}
