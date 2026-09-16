use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::{clear_popup, draw_labeled_input, focus_style, popup};

pub fn draw_issue_form(frame: &mut Frame, app: &App, area: Rect) {
    let (title, form) = match &app.overlay {
        Overlay::Create(form) => ("create story", form),
        Overlay::Edit(form) => ("edit issue", form),
        _ => return,
    };
    clear_popup(frame, area);
    let box_area = popup(area, 84, 24);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(title),
        box_area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .split(box_area);

    let issue_type = form
        .types
        .get(form.type_idx)
        .map(|t| t.name.as_str())
        .unwrap_or("Story");
    let priority = form
        .priorities
        .get(form.priority_idx)
        .map(|p| p.name.as_str())
        .unwrap_or("Medium");
    let sprint = form
        .sprints
        .get(form.sprint_idx)
        .map(|s| s.name.as_str())
        .unwrap_or("Backlog");

    picker_field(frame, chunks[0], "type (←/→)", issue_type, form.focus == 0);
    picker_field(
        frame,
        chunks[1],
        "priority (←/→)",
        priority,
        form.focus == 1,
    );
    picker_field(frame, chunks[2], "sprint (←/→)", sprint, form.focus == 2);
    draw_labeled_input(
        frame,
        chunks[3],
        "summary",
        &form.summary,
        form.focus == 3,
        false,
    );
    draw_labeled_input(
        frame,
        chunks[4],
        "story points",
        &form.story_points,
        form.focus == 4,
        false,
    );
    let desc_block = Block::default()
        .borders(Borders::ALL)
        .title("description (Tab to leave)")
        .border_style(focus_style(form.focus == 5));
    let inner = desc_block.inner(chunks[5]);
    frame.render_widget(desc_block, chunks[5]);
    frame.render_widget(&form.description, inner);
    frame.render_widget(
        Paragraph::new("Enter submit   Ctrl+Enter from description   Esc cancel"),
        chunks[6],
    );
}

pub fn draw_assign(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Assign(form) = &app.overlay else {
        return;
    };
    let box_area = popup(area, 60, 16);
    clear_popup(frame, box_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(box_area);
    let query_block = Block::default()
        .borders(Borders::ALL)
        .title("assign (type to search)");
    let inner = query_block.inner(chunks[0]);
    frame.render_widget(query_block, chunks[0]);
    draw_labeled_input(frame, inner, "query", &form.query, true, false);
    let items: Vec<ListItem> = form
        .users
        .iter()
        .enumerate()
        .map(|(i, user)| {
            let prefix = if i == form.selected { "> " } else { "  " };
            ListItem::new(format!("{prefix}{}", user.display_name))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("users")),
        chunks[1],
    );
    frame.render_widget(
        Paragraph::new("Enter assign   Ctrl+u unassign   Esc cancel"),
        chunks[2],
    );
}

pub fn draw_transition(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Transition { items, selected } = &app.overlay else {
        return;
    };
    let box_area = popup(area, 64, 16);
    clear_popup(frame, box_area);
    let lines: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let prefix = if i == *selected { "> " } else { "  " };
            let extra = if item.required_fields.is_empty() {
                String::new()
            } else {
                format!(" (required: {})", item.required_fields.join(", "))
            };
            ListItem::new(format!("{prefix}{} → {}{extra}", item.name, item.to_status))
        })
        .collect();
    frame.render_widget(
        List::new(lines).block(Block::default().borders(Borders::ALL).title("transition")),
        box_area,
    );
}

fn picker_field(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let line = Line::from(vec![
        Span::styled(
            format!("{label}: "),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), focus_style(focused)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
