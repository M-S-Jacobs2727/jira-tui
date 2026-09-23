use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::app::{App, Overlay};
use crate::ui::{clear_popup, draw_button, draw_labeled_input, focus_style, popup};

pub fn draw_issue_form(frame: &mut Frame, app: &App, area: Rect) {
    let (title, form) = match &app.overlay {
        Overlay::Create(form) => ("create issue", form),
        Overlay::Edit(form) => ("edit issue", form),
        _ => return,
    };
    clear_popup(frame, area);
    let show_parent = form.shows_parent();
    let show_sprint_points = form.shows_sprint_and_points();
    let mut height = 24u16;
    if show_sprint_points {
        height += 2; // sprint + story points
    }
    if show_parent {
        height += 1;
    }
    let box_area = popup(area, 84, height);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title(title),
        box_area,
    );

    let mut constraints = vec![
        Constraint::Length(1), // type
        Constraint::Length(1), // priority
    ];
    if show_sprint_points {
        constraints.push(Constraint::Length(1)); // sprint
    }
    constraints.push(Constraint::Length(1)); // assignee
    if show_parent {
        constraints.push(Constraint::Length(1)); // parent
    }
    constraints.push(Constraint::Length(1)); // summary
    if show_sprint_points {
        constraints.push(Constraint::Length(1)); // story points
    }
    constraints.extend([
        Constraint::Min(5),    // description
        Constraint::Length(1), // buttons
    ]);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
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

    let mut row = 0;
    picker_field(frame, chunks[row], "type (←/→)", issue_type, form.focus == row);
    row += 1;
    picker_field(
        frame,
        chunks[row],
        "priority (←/→)",
        priority,
        form.focus == row,
    );
    row += 1;
    if show_sprint_points {
        let sprint = form
            .sprints
            .get(form.sprint_idx)
            .map(|s| s.name.as_str())
            .unwrap_or("Backlog");
        picker_field(frame, chunks[row], "sprint (←/→)", sprint, form.focus == row);
        row += 1;
    }
    let assignee = form
        .assignees
        .get(form.assignee_idx)
        .map(|choice| choice.label.as_str())
        .unwrap_or("Unassigned");
    picker_field(
        frame,
        chunks[row],
        "assignee (←/→)",
        assignee,
        form.focus == row,
    );
    row += 1;

    if show_parent {
        let parent_label = if form.parent_required() {
            "parent * (Enter)"
        } else if form.parent_locked {
            "parent"
        } else {
            "parent (Enter)"
        };
        let parent_value = form
            .parent
            .as_ref()
            .map(|p| {
                if p.summary.is_empty() {
                    p.key.clone()
                } else {
                    format!("{}  {}", p.key, p.summary)
                }
            })
            .unwrap_or_else(|| {
                if form.parent_required() {
                    "(required)".into()
                } else {
                    "(none)".into()
                }
            });
        picker_field(
            frame,
            chunks[row],
            parent_label,
            &parent_value,
            form.focus == row,
        );
        row += 1;
    }

    draw_labeled_input(
        frame,
        chunks[row],
        "summary",
        &form.summary,
        form.focus == row,
        false,
    );
    row += 1;
    if show_sprint_points {
        draw_labeled_input(
            frame,
            chunks[row],
            "story points",
            &form.story_points,
            form.focus == row,
            false,
        );
        row += 1;
    }
    let desc_block = Block::default()
        .borders(Borders::ALL)
        .title("description")
        .border_style(focus_style(form.focus == row));
    let inner = desc_block.inner(chunks[row]);
    frame.render_widget(desc_block, chunks[row]);
    frame.render_widget(&form.description, inner);
    row += 1;
    let buttons = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[row]);
    draw_button(frame, buttons[0], "Submit (Enter)", form.focus == row);
    draw_button(frame, buttons[1], "Cancel (Esc)", form.focus == row + 1);
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

pub fn draw_parent_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::ParentPicker {
        query,
        results,
        selected,
        form,
        ..
    } = &app.overlay
    else {
        return;
    };
    let box_area = popup(area, 72, 18);
    clear_popup(frame, box_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(2),
        ])
        .split(box_area);
    let title = if form.parent_required() {
        "parent (type to search) *"
    } else {
        "parent (type to search)"
    };
    let query_block = Block::default().borders(Borders::ALL).title(title);
    let inner = query_block.inner(chunks[0]);
    frame.render_widget(query_block, chunks[0]);
    draw_labeled_input(frame, inner, "query", query, true, false);
    let items: Vec<ListItem> = results
        .iter()
        .enumerate()
        .map(|(i, parent)| {
            let prefix = if i == *selected { "> " } else { "  " };
            ListItem::new(format!("{prefix}{}  {}", parent.key, parent.summary))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("issues")),
        chunks[1],
    );
    let hint = if form.parent_required() {
        "Enter select   Esc cancel"
    } else {
        "Enter select   Ctrl+u clear   Esc cancel"
    };
    frame.render_widget(Paragraph::new(hint), chunks[2]);
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

pub fn draw_move_sprint(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::MoveSprint { items, selected } = &app.overlay else {
        return;
    };
    let height = (items.len() as u16).saturating_add(2).clamp(5, 18);
    let box_area = popup(area, 48, height);
    clear_popup(frame, box_area);
    let lines: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let prefix = if i == *selected { "> " } else { "  " };
            ListItem::new(format!("{prefix}{}", item.name))
        })
        .collect();
    frame.render_widget(
        List::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title("move to sprint / backlog"),
        ),
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
