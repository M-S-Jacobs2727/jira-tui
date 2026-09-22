use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::Fill;
use crate::ui::scroll_text::ScrollText;

pub fn draw(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::IssueDetail { issue, scroll } = &app.overlay else {
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
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Min(3),
        ])
        .margin(1)
        .split(area);

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
    ];

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(issue.key.as_str()),
        area,
    );
    frame.render_widget(Paragraph::new(header), chunks[0]);
    draw_section(
        frame,
        chunks[1],
        "description",
        &issue.description_text,
        *scroll,
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
    draw_section(frame, chunks[2], "comments", &comments, 0);
}

fn draw_section(frame: &mut Frame, area: Rect, title: &str, text: &str, scroll: u16) {
    let block = Block::default().borders(Borders::TOP).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(ScrollText::new(text, scroll), inner);
}
