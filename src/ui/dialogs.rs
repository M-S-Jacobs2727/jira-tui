use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::app::{App, Overlay, Screen};
use crate::jira::search::SortField;
use crate::ui::keys::{HELP_LINES, help_lines};
use crate::ui::{clear_popup, cursor_style, focus_style, popup};

pub fn draw_login(frame: &mut Frame, app: &App, area: Rect) {
    let Screen::Login(state) = &app.screen else {
        return;
    };
    clear_popup(frame, area);
    let box_area = popup(area, 92, 16);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(box_area);

    frame.render_widget(
        Block::default().borders(Borders::ALL).title("login"),
        box_area,
    );
    frame.render_widget(
        Paragraph::new("Press Enter to log in with Atlassian"),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Atlassian login link:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        chunks[2],
    );
    let auth_link = state.auth_url.as_deref().unwrap_or_else(|| {
        if state.error.is_some() {
            "(token service unavailable — press Enter to retry)"
        } else {
            "(contacting token service…)"
        }
    });
    frame.render_widget(
        Paragraph::new(Span::styled(
            auth_link.to_string(),
            Style::default().fg(Color::Cyan),
        ))
        .wrap(Wrap { trim: true }),
        chunks[4],
    );
    frame.render_widget(
        Paragraph::new("Enter start login   Ctrl+o open link   Ctrl+q quit"),
        chunks[5],
    );
}

pub fn draw_oauth_wait(frame: &mut Frame, app: &App, area: Rect) {
    let Screen::OauthWait { url } = &app.screen else {
        return;
    };
    clear_popup(frame, area);
    let box_area = popup(area, 90, 10);
    let lines = vec![
        Line::from(
            "Waiting for authorization. Open this Atlassian link if the browser did not launch:",
        ),
        Line::from(""),
        Line::from(Span::styled(url.clone(), Style::default().fg(Color::Cyan))),
        Line::from(""),
        Line::from("Ctrl+o open link   Esc cancel"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("oauth")),
        box_area,
    );
}

pub fn draw_site_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Screen::SitePicker { sites, selected } = &app.screen else {
        return;
    };
    clear_popup(frame, area);
    let box_area = popup(area, 70, 16);
    let items: Vec<ListItem> = sites
        .iter()
        .enumerate()
        .map(|(i, site)| {
            let prefix = if i == *selected { "> " } else { "  " };
            ListItem::new(format!("{prefix}{}  ({})", site.name, site.url))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("select site")),
        box_area,
    );
}

pub fn draw_board_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Screen::BoardPicker { boards, selected } = &app.screen else {
        return;
    };
    clear_popup(frame, area);
    let box_area = popup(area, 70, 18);
    let items: Vec<ListItem> = boards
        .iter()
        .enumerate()
        .map(|(i, board)| {
            let prefix = if i == *selected { "> " } else { "  " };
            ListItem::new(format!(
                "{prefix}{} [{}] {}",
                board.name,
                board.board_type,
                board.project_key.as_deref().unwrap_or("-")
            ))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title("select board")),
        box_area,
    );
}

pub fn draw_help(frame: &mut Frame, area: Rect) {
    let height = u16::try_from(HELP_LINES.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let box_area = popup(area, 72, height);
    clear_popup(frame, box_area);
    frame.render_widget(
        Paragraph::new(help_lines()).block(
            Block::default()
                .borders(Borders::ALL)
                .title("help  ?/Esc close"),
        ),
        box_area,
    );
}

pub fn draw_sort(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Sort { selected } = &app.overlay else {
        return;
    };
    let box_area = popup(area, 40, 14);
    clear_popup(frame, box_area);
    let items: Vec<ListItem> = SortField::ALL
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let prefix = if i == *selected { "> " } else { "  " };
            ListItem::new(format!("{prefix}{}", field.label()))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "sort ({})  d toggles",
            app.config.view.sort_dir.label()
        ))),
        box_area,
    );
}

pub fn draw_filter(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Filter(form) = &app.overlay else {
        return;
    };
    match &form.pane {
        crate::app::FilterPane::Menu => draw_filter_menu(frame, form, area),
        crate::app::FilterPane::Status { cursor, draft } => {
            draw_checklist(
                frame,
                area,
                "filter status",
                &form.status_options,
                draft,
                *cursor,
                form.loaded,
            );
        }
        crate::app::FilterPane::Type { cursor, draft } => {
            draw_checklist(
                frame,
                area,
                "filter type",
                &form.type_options,
                draft,
                *cursor,
                form.loaded,
            );
        }
        crate::app::FilterPane::Assignee { cursor, draft } => {
            draw_assignee_checklist(frame, form, area, draft, *cursor);
        }
    }
}

fn draw_filter_menu(frame: &mut Frame, form: &crate::app::FilterForm, area: Rect) {
    let box_area = popup(area, 64, 10);
    clear_popup(frame, box_area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(box_area);
    frame.render_widget(
        Block::default().borders(Borders::ALL).title("filter"),
        box_area,
    );
    menu_row(
        frame,
        chunks[0],
        "status",
        &form.status_label(),
        form.focus == 0,
    );
    menu_row(
        frame,
        chunks[1],
        "type",
        &form.type_label(),
        form.focus == 1,
    );
    menu_row(
        frame,
        chunks[2],
        "assignee",
        &form.assignee_label(),
        form.focus == 2,
    );
    menu_row(frame, chunks[3], "apply", "", form.focus == 3);
    frame.render_widget(
        Paragraph::new("Enter open or apply   Esc cancel"),
        chunks[5],
    );
}

fn menu_row(frame: &mut Frame, area: Rect, label: &str, value: &str, focused: bool) {
    let shown = if value.is_empty() {
        String::new()
    } else {
        format!("  {value}")
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("{label}:"),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(shown, focus_style(focused)),
        ])),
        area,
    );
}

fn draw_checklist(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    options: &[String],
    selected: &[String],
    cursor: usize,
    loaded: bool,
) {
    let box_area = popup(area, 56, 18);
    clear_popup(frame, box_area);
    let items = if options.is_empty() {
        vec![ListItem::new(if loaded {
            "no options"
        } else {
            "loading…"
        })]
    } else {
        options
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let mark = if selected.iter().any(|item| item == name) {
                    "[x]"
                } else {
                    "[ ]"
                };
                let prefix = if i == cursor { ">" } else { " " };
                ListItem::new(format!("{prefix} {mark} {name}"))
            })
            .collect()
    };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{title}   space toggle   Enter back")),
        ),
        box_area,
    );
}

fn draw_assignee_checklist(
    frame: &mut Frame,
    form: &crate::app::FilterForm,
    area: Rect,
    draft: &crate::jira::search::AssigneeFilter,
    cursor: usize,
) {
    let box_area = popup(area, 56, 18);
    clear_popup(frame, box_area);
    let mut rows = vec![checklist_row(cursor == 0, draft.unassigned, "Unassigned")];
    if form.users.is_empty() {
        rows.push(ListItem::new(if form.loaded {
            "  no assignees"
        } else {
            "  loading…"
        }));
    }
    for (i, user) in form.users.iter().enumerate() {
        let checked = draft.accounts.iter().any(|id| id == &user.account_id);
        let label = if form.self_account_id.as_deref() == Some(user.account_id.as_str()) {
            format!("{} (you)", user.display_name)
        } else {
            user.display_name.clone()
        };
        rows.push(checklist_row(cursor == i + 1, checked, &label));
    }
    frame.render_widget(
        List::new(rows).block(
            Block::default()
                .borders(Borders::ALL)
                .title("filter assignee   space toggle   Enter back"),
        ),
        box_area,
    );
}

fn checklist_row(cursor: bool, checked: bool, label: &str) -> ListItem<'static> {
    let mark = if checked { "[x]" } else { "[ ]" };
    let prefix = if cursor { ">" } else { " " };
    ListItem::new(format!("{prefix} {mark} {label}"))
}

pub fn draw_search(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Search { textarea } = &app.overlay else {
        return;
    };
    let box_area = popup(area, 60, 5);
    clear_popup(frame, box_area);
    let block = Block::default().borders(Borders::ALL).title("search");
    let inner = block.inner(box_area);
    frame.render_widget(block, box_area);
    frame.render_widget(textarea, inner);
}

pub fn draw_command(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::Command { input } = &app.overlay else {
        return;
    };
    let box_area = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(3),
        width: area.width,
        height: 3.min(area.height),
    };
    clear_popup(frame, box_area);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(":", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(input.clone()),
            Span::styled("█", cursor_style()),
        ]))
        .block(Block::default().borders(Borders::ALL).title("command")),
        box_area,
    );
}

pub fn draw_delete_confirm(frame: &mut Frame, app: &App, area: Rect) {
    let Overlay::DeleteConfirm { key } = &app.overlay else {
        return;
    };
    let box_area = popup(area, 48, 6);
    clear_popup(frame, box_area);
    frame.render_widget(
        Paragraph::new(format!("Delete {key}?  y confirm   n / Esc cancel"))
            .block(Block::default().borders(Borders::ALL).title("confirm")),
        box_area,
    );
}

pub fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let (text, style) = if !app.status.is_empty() {
        let style = if app.status_is_error {
            Style::default()
                .fg(Color::Red)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .bg(Color::Black)
                .add_modifier(Modifier::BOLD)
        };
        let prefix = if app.status_is_error {
            "error"
        } else if app.loading || app.status.contains("loading") || app.status.contains("…") {
            "busy"
        } else {
            "info"
        };
        (format!(" {prefix}: {} ", app.status), style)
    } else if app.loading {
        (
            " busy: working… ".to_string(),
            Style::default().fg(Color::Yellow).bg(Color::Black),
        )
    } else {
        (
            " ready   ? help   q quit ".to_string(),
            Style::default().fg(Color::Gray).bg(Color::Black),
        )
    };
    let width = area.width as usize;
    let padded = format!("{text:<width$}");
    frame.render_widget(Paragraph::new(Span::styled(padded, style)), area);
}
