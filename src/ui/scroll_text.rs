use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;

/// Scrollable wrapped text that writes every cell in `area`.
///
/// `Paragraph::scroll` only paints the graphemes of each visible line. It also
/// stores control characters (tabs, etc.) whose terminal width is not 1. After
/// that the crossterm backend and ratatui's buffers disagree, and each scrolled
/// line leaves a leftover glyph. Filling with spaces first cannot fix it: the
/// current buffer is already reset to spaces before every frame.
pub struct ScrollText<'a> {
    text: &'a str,
    scroll: u16,
}

impl<'a> ScrollText<'a> {
    pub fn new(text: &'a str, scroll: u16) -> Self {
        Self { text, scroll }
    }

    pub fn line_count(text: &str, width: usize) -> usize {
        wrap_text(text, width).len()
    }
}

impl Widget for ScrollText<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = area.width as usize;
        let rows = wrap_text(self.text, width);
        let start = usize::from(self.scroll);
        let height = usize::from(area.height);
        let more_up = start > 0;
        let more_down = start.saturating_add(height) < rows.len();
        for row in 0..area.height {
            let y = area.y + row;
            for x in area.left()..area.right() {
                buf[(x, y)].reset();
            }
            if let Some(line) = rows.get(start + usize::from(row)) {
                buf.set_stringn(area.x, y, line, width, Style::default());
            }
            // Continuation markers in the rightmost cell of the first/last row.
            if width > 0 {
                let mark = if row == 0 && more_up {
                    Some("↑")
                } else if usize::from(row) + 1 == height && more_down {
                    Some("↓")
                } else {
                    None
                };
                if let Some(mark) = mark {
                    buf.set_stringn(area.right() - 1, y, mark, 1, Style::default());
                }
            }
        }
    }
}

fn display_width(text: &str) -> usize {
    Line::from(text).width()
}

fn sanitize_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '\t' {
            out.push_str("    ");
        } else if ch == '\n' || !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for raw in text.split('\n') {
        let line = sanitize_line(raw);
        if line.is_empty() {
            rows.push(String::new());
            continue;
        }
        wrap_line(&mut rows, &line, width);
    }
    rows
}

fn wrap_line(rows: &mut Vec<String>, line: &str, width: usize) {
    let mut current = String::new();
    let mut current_width = 0usize;
    for token in line.split_inclusive(char::is_whitespace) {
        let token_width = display_width(token);
        if token_width > width {
            if !current.is_empty() {
                rows.push(std::mem::take(&mut current));
                current_width = 0;
            }
            for ch in token.chars() {
                let chunk = ch.to_string();
                let chunk_width = display_width(&chunk);
                if current_width + chunk_width > width && current_width > 0 {
                    rows.push(std::mem::take(&mut current));
                    current_width = 0;
                }
                current.push(ch);
                current_width += chunk_width;
            }
            continue;
        }
        if current_width + token_width > width && current_width > 0 {
            rows.push(std::mem::take(&mut current));
            current_width = 0;
        }
        current.push_str(token);
        current_width += token_width;
    }
    if !current.is_empty() {
        rows.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Cell;

    #[test]
    fn wraps_on_word_boundaries() {
        assert_eq!(
            wrap_text("hello world friends", 10),
            vec!["hello ", "world ", "friends"]
        );
    }

    #[test]
    fn keeps_blank_logical_lines() {
        assert_eq!(wrap_text("a\n\nb", 10), vec!["a", "", "b"]);
    }

    #[test]
    fn hard_wraps_long_tokens() {
        assert_eq!(wrap_text("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn overwrites_every_cell_in_the_area() {
        let area = Rect::new(0, 0, 8, 3);
        let mut buf = Buffer::filled(area, Cell::new("X"));
        ScrollText::new("hi\n\nbye", 0).render(area, &mut buf);
        assert_eq!(
            buf,
            Buffer::with_lines(["hi      ", "        ", "bye     "])
        );
    }

    #[test]
    fn scroll_replaces_previous_rows() {
        let area = Rect::new(0, 0, 8, 2);
        let mut buf = Buffer::filled(area, Cell::new("X"));
        ScrollText::new("one\ntwo\nthree", 1).render(area, &mut buf);
        // ↑ on the first visible row when scrolled down.
        assert_eq!(buf, Buffer::with_lines(["two    ↑", "three   "]));
    }

    #[test]
    fn shows_down_marker_when_more_below() {
        let area = Rect::new(0, 0, 8, 2);
        let mut buf = Buffer::filled(area, Cell::new("X"));
        ScrollText::new("one\ntwo\nthree", 0).render(area, &mut buf);
        assert_eq!(buf, Buffer::with_lines(["one     ", "two    ↓"]));
    }

    #[test]
    fn drops_tabs_instead_of_storing_them() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        ScrollText::new("a\tb", 0).render(area, &mut buf);
        assert_eq!(buf, Buffer::with_lines(["a    b    "]));
        assert_ne!(buf[(1, 0)].symbol(), "\t");
    }
}
