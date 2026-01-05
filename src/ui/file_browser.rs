use std::path::PathBuf;
use std::fs;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Widget},
};

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

pub struct FileBrowser {
    pub current_dir: PathBuf,
    pub entries: Vec<DirEntry>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub error: Option<String>,
    pub parent_selected: bool, // True when ".." is selected
}

impl FileBrowser {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut browser = Self {
            current_dir: home,
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            error: None,
            parent_selected: false,
        };
        browser.refresh();
        browser
    }

    pub fn refresh(&mut self) {
        self.entries.clear();
        self.error = None;

        match fs::read_dir(&self.current_dir) {
            Ok(read_dir) => {
                let mut dirs: Vec<DirEntry> = Vec::new();
                let mut files: Vec<DirEntry> = Vec::new();

                for entry in read_dir.flatten() {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();

                    // Skip hidden files/folders
                    if name.starts_with('.') {
                        continue;
                    }

                    let is_dir = path.is_dir();
                    let entry = DirEntry { name, path, is_dir };

                    if is_dir {
                        dirs.push(entry);
                    } else {
                        files.push(entry);
                    }
                }

                // Sort alphabetically
                dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                // Directories first, then files
                self.entries = dirs;
                self.entries.extend(files);
            }
            Err(e) => {
                self.error = Some(format!("Cannot read directory: {}", e));
            }
        }

        // Reset selection
        self.selected = 0;
        self.scroll_offset = 0;
        self.parent_selected = false;
    }

    pub fn move_up(&mut self) {
        if self.parent_selected {
            // Already at top, can't go higher
            return;
        }

        if self.selected > 0 {
            self.selected -= 1;
            // Adjust scroll if selection goes above visible area
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        } else {
            // At first entry, move to parent directory option
            self.parent_selected = true;
            self.scroll_offset = 0;
        }
    }

    pub fn move_down(&mut self, visible_height: usize) {
        if self.parent_selected {
            // Move from parent to first entry
            self.parent_selected = false;
            self.selected = 0;
            return;
        }

        if !self.entries.is_empty() && self.selected < self.entries.len() - 1 {
            self.selected += 1;
            // Adjust scroll if selection goes below visible area
            if self.selected >= self.scroll_offset + visible_height {
                self.scroll_offset = self.selected - visible_height + 1;
            }
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let old_dir = self.current_dir.clone();
            self.current_dir = parent.to_path_buf();
            self.refresh();

            // Try to select the directory we came from
            if let Some(old_name) = old_dir.file_name() {
                let old_name = old_name.to_string_lossy();
                if let Some(idx) = self.entries.iter().position(|e| e.name == old_name) {
                    self.selected = idx;
                }
            }
        }
    }

    /// Navigate into the selected directory (Enter key)
    pub fn enter(&mut self) -> bool {
        // If ".." is selected, go to parent
        if self.parent_selected {
            self.go_up();
            return true;
        }

        if let Some(entry) = self.entries.get(self.selected) {
            if entry.is_dir {
                self.current_dir = entry.path.clone();
                self.refresh();
                return true;
            }
        }
        false
    }

    /// Get the path to load (Tab key) - returns selected folder or current dir
    pub fn get_load_path(&self) -> PathBuf {
        // If ".." is selected, return parent directory
        if self.parent_selected {
            return self.current_dir.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| self.current_dir.clone());
        }

        if let Some(entry) = self.entries.get(self.selected) {
            if entry.is_dir {
                return entry.path.clone();
            }
        }
        // Default to current directory
        self.current_dir.clone()
    }

    #[allow(dead_code)]
    pub fn adjust_scroll(&mut self, visible_height: usize) {
        // Ensure selected item is visible
        if self.selected >= self.scroll_offset + visible_height {
            self.scroll_offset = self.selected - visible_height + 1;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
    }
}

impl Default for FileBrowser {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FileBrowserWidget<'a> {
    browser: &'a FileBrowser,
    title: &'a str,
}

impl<'a> FileBrowserWidget<'a> {
    pub fn new(browser: &'a FileBrowser) -> Self {
        Self {
            browser,
            title: " Select Folder ",
        }
    }

    pub fn with_title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }
}

impl Widget for FileBrowserWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate dialog size (70% of screen, max 80x30)
        let width = (area.width * 70 / 100).min(80).max(40);
        let height = (area.height * 70 / 100).min(30).max(10);
        let x = (area.width.saturating_sub(width)) / 2;
        let y = (area.height.saturating_sub(height)) / 2;

        let dialog_area = Rect::new(x, y, width, height);

        // Clear background
        Clear.render(dialog_area, buf);

        // Draw border
        let block = Block::default()
            .title(self.title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(dialog_area);
        block.render(dialog_area, buf);

        // Current path header
        let path_str = self.browser.current_dir.display().to_string();
        let path_style = Style::default().fg(Color::Yellow);
        let truncated_path = if path_str.len() > (inner.width as usize - 2) {
            format!("...{}", &path_str[path_str.len().saturating_sub(inner.width as usize - 5)..])
        } else {
            path_str
        };
        buf.set_string(inner.x + 1, inner.y, &truncated_path, path_style);

        // Separator
        let sep = "─".repeat(inner.width as usize - 2);
        buf.set_string(inner.x + 1, inner.y + 1, &sep, Style::default().fg(Color::DarkGray));

        // Error message if any
        if let Some(ref error) = self.browser.error {
            let error_style = Style::default().fg(Color::Red);
            buf.set_string(inner.x + 1, inner.y + 2, error, error_style);
            return;
        }

        // List entries
        let list_area = Rect::new(inner.x, inner.y + 2, inner.width, inner.height.saturating_sub(4));
        let visible_count = list_area.height as usize;

        // Parent directory entry
        let parent_style = if self.browser.parent_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::Blue)
        };

        if self.browser.scroll_offset == 0 {
            let indicator = if self.browser.parent_selected { "> " } else { "  " };

            // Fill background if selected
            if self.browser.parent_selected {
                let fill = " ".repeat(list_area.width as usize - 2);
                buf.set_string(list_area.x + 1, list_area.y, &fill, parent_style);
            }

            let parent_line = Line::from(vec![
                Span::styled(indicator, parent_style),
                Span::styled("..", parent_style.add_modifier(Modifier::BOLD)),
                Span::styled(" (parent directory)", if self.browser.parent_selected {
                    parent_style
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
            ]);
            buf.set_line(list_area.x + 1, list_area.y, &parent_line, list_area.width - 2);
        }

        let entries_start_y = if self.browser.scroll_offset == 0 { 1 } else { 0 };
        let scroll = self.browser.scroll_offset.saturating_sub(if self.browser.scroll_offset > 0 { 1 } else { 0 });

        for (i, entry) in self.browser.entries.iter().skip(scroll).take(visible_count - entries_start_y).enumerate() {
            let y_pos = list_area.y + entries_start_y as u16 + i as u16;
            if y_pos >= list_area.y + list_area.height {
                break;
            }

            let actual_idx = i + scroll;
            // Don't show any entry as selected if parent is selected
            let is_selected = !self.browser.parent_selected && actual_idx == self.browser.selected;

            let icon = if entry.is_dir { "" } else { "" };

            let style = if is_selected {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else if entry.is_dir {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::White)
            };

            let name = if entry.name.len() > (list_area.width as usize - 6) {
                format!("{}...", &entry.name[..list_area.width as usize - 9])
            } else {
                entry.name.clone()
            };

            let indicator = if is_selected { ">" } else { " " };
            let line_text = format!("{} {} {}", indicator, icon, name);

            // Fill the entire line for selection highlight
            if is_selected {
                let fill = " ".repeat(list_area.width as usize - 2);
                buf.set_string(list_area.x + 1, y_pos, &fill, style);
            }

            buf.set_string(list_area.x + 1, y_pos, &line_text, style);
        }

        // Footer with controls
        let footer_y = inner.y + inner.height - 2;
        let sep = "─".repeat(inner.width as usize - 2);
        buf.set_string(inner.x + 1, footer_y, &sep, Style::default().fg(Color::DarkGray));

        let controls = Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Yellow)),
            Span::styled(":Open  ", Style::default().fg(Color::DarkGray)),
            Span::styled("l/Tab", Style::default().fg(Color::Yellow)),
            Span::styled(":Load  ", Style::default().fg(Color::DarkGray)),
            Span::styled("←", Style::default().fg(Color::Yellow)),
            Span::styled(":Back  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc", Style::default().fg(Color::Yellow)),
            Span::styled(":Cancel", Style::default().fg(Color::DarkGray)),
        ]);
        buf.set_line(inner.x + 1, footer_y + 1, &controls, inner.width - 2);
    }
}
