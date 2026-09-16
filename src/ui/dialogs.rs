use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::app::{App, Overlay, Screen};
use crate::auth::REDIRECT_URI;
use crate::jira::search::SortField;
use crate::ui::keys::help_lines;
use crate::ui::{clear_popup, cursor_style, draw_labeled_input, focus_style, popup};

pub fn draw_login(frame: &mut Frame, app: &App, area: Rect) {
    let Screen::Login(state) = &app.screen else {
        return;
    };
    clear_popup(frame, area);
    let box_area = popup(area, 92, 20);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
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
        Paragraph::new("Sign in with Atlassian OAuth 2.0 (3LO)"),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("Register callback {REDIRECT_URI}")),
        chunks[1],
    );
    draw_labeled_input(
        frame,
        chunks[3],
        "client id",
        &state.client_id,
        state.focus == 0,
        false,
    );
    draw_labeled_input(
        frame,
        chunks[4],
        "client secret",
        &state.client_secret,
        state.focus == 1,
        true,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "Atlassian login link:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        chunks[6],
    );
    let auth_link = state
        .auth_url
        .as_deref()
        .unwrap_or("(enter a client id to generate the Atlassian login link)");
    frame.render_widget(
        Paragraph::new(Span::styled(
            auth_link.to_string(),
            Style::default().fg(Color::Cyan),
        ))
        .wrap(Wrap { trim: true }),
        chunks[7],
    );
    frame.render_widget(
        Paragraph::new("Tab fields   Enter start login   Ctrl+o open link   Ctrl+q quit"),
        chunks[8],
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
    clear_popup(frame, area);
    let box_area = popup(area, 72, 28);
    frame.render_widget(
        Paragraph::new(help_lines()).block(Block::default().borders(Borders::ALL).title("help")),
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
    let box_area = popup(area, 64, 12);
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
    draw_labeled_input(
        frame,
        chunks[0],
        "statuses (csv)",
        &form.statuses,
        form.focus == 0,
        false,
    );
    draw_labeled_input(
        frame,
        chunks[1],
        "types (csv)",
        &form.types,
        form.focus == 1,
        false,
    );
    let assignee = form.assignee_label();
    let assignee_style = if form.focus == 2 {
        focus_style(true)
    } else {
        Style::default()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "assignee (←/→): ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(assignee, assignee_style),
        ])),
        chunks[2],
    );
    draw_labeled_input(
        frame,
        chunks[3],
        "named account/id",
        &form.named,
        form.focus == 3,
        false,
    );
    frame.render_widget(
        Paragraph::new("Enter apply (saved)   Esc cancel"),
        chunks[5],
    );
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
