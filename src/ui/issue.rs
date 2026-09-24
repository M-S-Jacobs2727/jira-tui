use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, DetailFocus, Overlay};
use crate::jira::models::Issue;
use crate::ui::Fill;
use crate::ui::board::{
    cell_value, resolve_column_widths, summary_leads_to_type, with_leaders,
};
use crate::ui::scroll_text::ScrollText;

const COLLAPSED_HEIGHT: u16 = 3; // top border + 2 content lines

pub fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let Overlay::IssueDetail {
        issue,
        children,
        child_selected,
        children_scroll,
        focus,
        desc_scroll,
        comment_scroll,
    } = &app.overlay
    else {
        return;
    };
    // Copy values we need after we stop borrowing overlay for layout/mutation.
    let focus = *focus;
    let child_selected = *child_selected;
    let mut children_scroll = *children_scroll;
    let mut desc_scroll = *desc_scroll;
    let mut comment_scroll = *comment_scroll;
    let children = children.clone();
    let issue = issue.clone();

    frame.render_widget(Fill, area);

    let Some(issue) = issue else {
        frame.render_widget(
            Paragraph::new("loading issue…")
                .block(Block::default().borders(Borders::ALL).title("issue")),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(accordion_constraints(focus))
        .margin(1)
        .split(area);

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(issue.key.as_str()),
        area,
    );
    frame.render_widget(Paragraph::new(header_lines(&issue)), chunks[0]);

    let desc_focused = focus == DetailFocus::Description;
    let children_focused = focus == DetailFocus::Children;
    let comments_focused = focus == DetailFocus::Comments;

    let desc_inner = section_inner(chunks[1]);
    let desc_total = ScrollText::line_count(&issue.description_text, desc_inner.width as usize);
    desc_scroll = clamp_u16(desc_scroll, max_scroll(desc_total, desc_inner.height));
    draw_section(
        frame,
        chunks[1],
        &section_title("description", desc_focused),
        &issue.description_text,
        desc_scroll,
        desc_focused,
    );

    let child_inner = section_inner(chunks[2]);
    let child_rows = child_inner.height as usize;
    children_scroll = clamp_children_scroll(children_scroll, children.len(), child_rows);
    if !children.is_empty() && child_rows > 0 {
        if child_selected < children_scroll {
            children_scroll = child_selected;
        } else if child_selected >= children_scroll + child_rows {
            children_scroll = child_selected.saturating_add(1).saturating_sub(child_rows);
        }
        children_scroll = clamp_children_scroll(children_scroll, children.len(), child_rows);
    }
    draw_children(
        frame,
        chunks[2],
        &children,
        child_selected,
        children_scroll,
        children_focused,
        &app.config.columns(),
        app.config.story_points_field(),
    );

    let comments = comments_text(&issue);
    let comment_inner = section_inner(chunks[3]);
    let comment_total = ScrollText::line_count(&comments, comment_inner.width as usize);
    comment_scroll = clamp_u16(
        comment_scroll,
        max_scroll(comment_total, comment_inner.height),
    );
    draw_section(
        frame,
        chunks[3],
        &section_title("comments", comments_focused),
        &comments,
        comment_scroll,
        comments_focused,
    );

    // Persist clamped scrolls and viewport size for key handlers.
    match focus {
        DetailFocus::Description => {
            app.detail_rows = desc_inner.height as usize;
            app.detail_max_scroll = max_scroll(desc_total, desc_inner.height);
        }
        DetailFocus::Children => {
            app.detail_rows = child_rows;
            app.detail_max_scroll = children.len().saturating_sub(child_rows.max(1)) as u16;
        }
        DetailFocus::Comments => {
            app.detail_rows = comment_inner.height as usize;
            app.detail_max_scroll = max_scroll(comment_total, comment_inner.height);
        }
    }
    if let Overlay::IssueDetail {
        desc_scroll: ds,
        comment_scroll: cs,
        children_scroll: chs,
        ..
    } = &mut app.overlay
    {
        *ds = desc_scroll;
        *cs = comment_scroll;
        *chs = children_scroll;
    }
}

fn accordion_constraints(focus: DetailFocus) -> [Constraint; 4] {
    let collapsed = Constraint::Length(COLLAPSED_HEIGHT);
    let expanded = Constraint::Min(3);
    match focus {
        DetailFocus::Description => [Constraint::Length(4), expanded, collapsed, collapsed],
        DetailFocus::Children => [Constraint::Length(4), collapsed, expanded, collapsed],
        DetailFocus::Comments => [Constraint::Length(4), collapsed, collapsed, expanded],
    }
}

fn section_inner(area: Rect) -> Rect {
    Block::default().borders(Borders::TOP).inner(area)
}

fn section_title(name: &str, focused: bool) -> String {
    if focused {
        format!("{name} *")
    } else {
        name.to_string()
    }
}

fn comments_text(issue: &Issue) -> String {
    if issue.comments.is_empty() {
        "no comments".to_string()
    } else {
        issue
            .comments
            .iter()
            .map(|c| format!("{}  {}\n{}\n", c.author, c.created, c.body))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn max_scroll(total: usize, height: u16) -> u16 {
    total.saturating_sub(height as usize) as u16
}

fn clamp_u16(scroll: u16, max: u16) -> u16 {
    scroll.min(max)
}

fn clamp_children_scroll(scroll: usize, len: usize, rows: usize) -> usize {
    if len == 0 || rows == 0 {
        return 0;
    }
    scroll.min(len.saturating_sub(rows))
}

fn header_lines(issue: &Issue) -> Vec<Line<'static>> {
    let type_val = issue.issue_type.clone();
    let status_val = issue.status.clone();
    let priority_val = issue
        .priority
        .clone()
        .unwrap_or_else(|| "-".into());
    let points_val = issue
        .story_points
        .map(|n| {
            if n.fract() == 0.0 {
                format!("{n:.0}")
            } else {
                format!("{n}")
            }
        })
        .unwrap_or_else(|| "-".into());
    let assignee_val = issue
        .assignee
        .as_ref()
        .map(|u| u.display_name.clone())
        .unwrap_or_else(|| "Unassigned".into());

    let type_part = format!("Type: {type_val}");
    let status_start = display_width(&type_part) + 3;
    let priority_gap = 10usize;
    let points_part = format!("Points: {points_val}");
    let assignee_pad = status_start.saturating_sub(display_width(&points_part));

    let parent_line = match &issue.parent {
        Some(parent) if !parent.summary.is_empty() => {
            labeled_parent(format!("{}  {}", parent.key, parent.summary))
        }
        Some(parent) => labeled_parent(parent.key.clone()),
        None => Line::from(vec![
            field_name("Parent"),
            Span::raw(": -"),
        ]),
    };

    vec![
        Line::from(Span::styled(
            format!("{}  {}", issue.key, issue.summary),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            field_name("Type"),
            Span::raw(format!(": {type_val}")),
            Span::raw("   "),
            field_name("Status"),
            Span::raw(format!(": {status_val}")),
            Span::raw(" ".repeat(priority_gap)),
            field_name("Priority"),
            Span::raw(format!(": {priority_val}")),
        ]),
        Line::from(vec![
            field_name("Points"),
            Span::raw(format!(": {points_val}")),
            Span::raw(" ".repeat(assignee_pad.max(1))),
            field_name("Assignee"),
            Span::raw(format!(": {assignee_val}")),
        ]),
        parent_line,
    ]
}

fn labeled_parent(value: String) -> Line<'static> {
    Line::from(vec![
        field_name("Parent"),
        Span::raw(" (p)"),
        Span::raw(format!(": {value}")),
    ])
}

fn field_name(name: &str) -> Span<'static> {
    Span::styled(
        name.to_string(),
        Style::default().add_modifier(Modifier::UNDERLINED),
    )
}

fn display_width(text: &str) -> usize {
    Line::from(text).width()
}

fn draw_children(
    frame: &mut Frame,
    area: Rect,
    children: &[Issue],
    selected: usize,
    scroll: usize,
    focused: bool,
    columns: &[String],
    story_points_field: Option<&str>,
) {
    let title = if focused {
        "children (Enter open · c create) *"
    } else {
        "children (c create)"
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .border_style(focus_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if children.is_empty() {
        frame.render_widget(Paragraph::new("no children"), inner);
        return;
    }
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let prefix_width = 2u16;
    let available = inner.width.saturating_sub(prefix_width);
    let widths = resolve_column_widths(columns, available);
    let lead_summary = summary_leads_to_type(columns);
    let rows = inner.height as usize;
    let end = (scroll + rows).min(children.len());

    let lines: Vec<Line> = children[scroll..end]
        .iter()
        .enumerate()
        .map(|(offset, child)| {
            let i = scroll + offset;
            let prefix = if focused && i == selected {
                "> "
            } else {
                "  "
            };
            let row = format_child_row(child, columns, &widths, lead_summary, story_points_field);
            let text = format!("{prefix}{row}");
            if focused && i == selected {
                Line::from(Span::styled(
                    text,
                    Style::default().add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(text)
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn format_child_row(
    issue: &Issue,
    columns: &[String],
    widths: &[u16],
    lead_summary: bool,
    story_points_field: Option<&str>,
) -> String {
    let mut parts = Vec::with_capacity(columns.len());
    for (idx, col) in columns.iter().enumerate() {
        let width = widths.get(idx).copied().unwrap_or(12) as usize;
        let mut value = cell_value(issue, col, story_points_field);
        if lead_summary && col == "summary" {
            value = with_leaders(&value, width);
        } else {
            value = pad_or_truncate(&value, width);
        }
        parts.push(value);
    }
    parts.join(" ")
}

fn pad_or_truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() > width {
        return chars.into_iter().take(width).collect();
    }
    let mut out = text.to_string();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn draw_section(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    text: &str,
    scroll: u16,
    focused: bool,
) {
    let inner_probe = Block::default().borders(Borders::TOP).inner(area);
    let total = ScrollText::line_count(text, inner_probe.width as usize);
    let height = inner_probe.height as usize;
    let more_up = scroll > 0;
    let more_down = (scroll as usize).saturating_add(height) < total;
    let title = match (more_up, more_down) {
        (true, true) => format!("{title} ↑↓"),
        (true, false) => format!("{title} ↑"),
        (false, true) => format!("{title} ↓"),
        (false, false) => title.to_string(),
    };
    let block = Block::default()
        .borders(Borders::TOP)
        .title(title)
        .border_style(focus_style(focused));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(ScrollText::new(text, scroll), inner);
}

fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default().fg(ratatui::style::Color::Cyan)
    } else {
        Style::default()
    }
}
