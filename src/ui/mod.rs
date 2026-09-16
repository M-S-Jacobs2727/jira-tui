pub mod board;
pub mod dialogs;
pub mod forms;
pub mod issue;
pub mod keys;
pub mod scroll_text;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::{App, Overlay, Screen};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());
    let content = chunks[0];
    let status = chunks[1];

    match &app.screen {
        Screen::Login(_) => dialogs::draw_login(frame, app, content),
        Screen::OauthWait { .. } => dialogs::draw_oauth_wait(frame, app, content),
        Screen::SitePicker { .. } => dialogs::draw_site_picker(frame, app, content),
        Screen::BoardPicker { .. } => dialogs::draw_board_picker(frame, app, content),
        Screen::Main => match &app.overlay {
            Overlay::IssueDetail { .. } => issue::draw(frame, app, content),
            Overlay::Create(_) | Overlay::Edit(_) => forms::draw_issue_form(frame, app, content),
            Overlay::Help => dialogs::draw_help(frame, content),
            Overlay::None => board::draw(frame, app, content),
            Overlay::Sort { .. } => {
                board::draw(frame, app, content);
                dialogs::draw_sort(frame, app, content);
            }
            Overlay::Filter(_) => {
                board::draw(frame, app, content);
                dialogs::draw_filter(frame, app, content);
            }
            Overlay::Search { .. } => {
                board::draw(frame, app, content);
                dialogs::draw_search(frame, app, content);
            }
            Overlay::Command { .. } => {
                board::draw(frame, app, content);
                dialogs::draw_command(frame, app, content);
            }
            Overlay::DeleteConfirm { .. } => {
                board::draw(frame, app, content);
                dialogs::draw_delete_confirm(frame, app, content);
            }
            Overlay::Assign(_) => {
                board::draw(frame, app, content);
                forms::draw_assign(frame, app, content);
            }
            Overlay::Transition { .. } => {
                board::draw(frame, app, content);
                forms::draw_transition(frame, app, content);
            }
        },
    }
    frame.render_widget(Fill, status);
    dialogs::draw_status(frame, status, app);
}

pub fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2).max(1));
    let height = height.min(area.height.saturating_sub(2).max(1));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Writes real spaces so the terminal backend will overwrite leftover glyphs.
/// `Clear` only resets cells to empty, and empty cells are skipped on the next diff.
pub struct Fill;

impl Widget for Fill {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                buf[(x, y)].set_symbol(" ");
                buf[(x, y)].set_style(Style::default());
            }
        }
    }
}

pub fn clear_popup(frame: &mut Frame, area: Rect) {
    frame.render_widget(Fill, area);
}

pub fn cursor_style() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

pub fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

pub fn draw_labeled_input(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    focused: bool,
    secret: bool,
) {
    let shown = if secret {
        "*".repeat(value.chars().count())
    } else {
        value.to_string()
    };
    let mut spans = vec![
        Span::styled(
            format!("{label}: "),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(shown, focus_style(focused)),
    ];
    if focused {
        spans.push(Span::styled("█", cursor_style()));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
