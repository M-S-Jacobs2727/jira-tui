use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, DetailFocus, Overlay};
use crate::ui::Fill;
use crate::ui::scroll_text::ScrollText;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::IssueDetail {
        issue,
        children,
        child_selected,
        focus,
        scroll,
    } = &app.overlay
    else {
        return;
    };
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
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(children_height(children.len())),
            Constraint::Min(3),
        ])
        .margin(1)
        .split(area);

    let parent_line = match &issue.parent {
        Some(parent) if !parent.summary.is_empty() => {
            format!("Parent: {}  {}  (p)", parent.key, parent.summary)
        }
        Some(parent) => format!("Parent: {}  (p)", parent.key),
        None => "Parent: -".into(),
    };

    let header = vec![
        Line::from(Span::styled(
            format!("{}  {}", issue.key, issue.summary),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "Type: {}   Status: {}   Priority: {}",
            issue.issue_type,
            issue.status,
            issue.priority.as_deref().unwrap_or("-")
        )),
        Line::from(format!(
            "Assignee: {}   Points: {}",
            issue
                .assignee
                .as_ref()
                .map(|u| u.display_name.as_str())
                .unwrap_or("Unassigned"),
            issue
                .story_points
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into())
        )),
        Line::from(parent_line),
    ];

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(issue.key.as_str()),
        area,
    );
    frame.render_widget(Paragraph::new(header), chunks[0]);
    let desc_focused = *focus == DetailFocus::Description;
    draw_section(
        frame,
        chunks[1],
        if desc_focused {
            "description (Tab children)"
        } else {
            "description"
        },
        &issue.description_text,
        *scroll,
        desc_focused,
    );

    draw_children(
        frame,
        chunks[2],
        children,
        *child_selected,
        *focus == DetailFocus::Children,
    );

    let comments = if issue.comments.is_empty() {
        "no comments".to_string()
    } else {
        issue
            .comments
            .iter()
            .map(|c| format!("{}  {}\n{}\n", c.author, c.created, c.body))
            .collect::<Vec<_>>()
            .join("\n")
    };
    draw_section(frame, chunks[3], "comments", &comments, 0, false);
}

fn children_height(len: usize) -> u16 {
    ((len as u16).saturating_add(2)).clamp(3, 10)
}

fn draw_children(
    frame: &mut Frame,
    area: Rect,
    children: &[crate::jira::models::Issue],
    selected: usize,
    focused: bool,
) {
    let title = if focused {
        "children (Enter open · c create) *"
    } else {
        "children (Tab · c create)"
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
    let lines: Vec<Line> = children
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let prefix = if focused && i == selected {
                "> "
            } else {
                "  "
            };
            let text = format!(
                "{prefix}{}  [{}]  {}",
                child.key, child.issue_type, child.summary
            );
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
