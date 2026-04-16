use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, BorderType, Clear, Paragraph, Wrap};
use crate::theme::Palette;

pub fn render_modal(
    frame: &mut Frame,
    title: &str,
    lines: Vec<Line<'_>>,
    width_percent: u16,
    height_percent: u16,
    palette: &Palette,
) {
    let area = centered_rect(width_percent, height_percent, frame.area());
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(palette.focus))
        .style(Style::default().bg(palette.bg_panel()).fg(palette.text));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let content = Rect {
        x: inner.x.saturating_add(1),
        y: inner.y,
        width: inner.width.saturating_sub(2),
        height: inner.height,
    };
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: true });
    frame.render_widget(paragraph, content);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
