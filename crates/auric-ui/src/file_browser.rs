use std::path::{Path, PathBuf};

pub struct FileBrowser {
    current_dir: PathBuf,
    entries: Vec<DirEntry>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub path_input: String,
    pub input_focused: bool,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

impl FileBrowser {
    pub fn new(start_dir: &Path) -> Self {
        let mut browser = Self {
            current_dir: start_dir.to_path_buf(),
            entries: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            path_input: start_dir.display().to_string(),
            input_focused: false,
        };
        browser.refresh_entries();
        browser
    }

    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    pub fn entries(&self) -> &[DirEntry] {
        &self.entries
    }

    pub fn selected_entry(&self) -> Option<&DirEntry> {
        self.entries.get(self.selected)
    }

    pub fn selected_path(&self) -> PathBuf {
        self.selected_entry()
            .map(|e| e.path.clone())
            .unwrap_or_else(|| self.current_dir.clone())
    }

    pub fn refresh_entries(&mut self) {
        self.entries.clear();
        if let Ok(read_dir) = std::fs::read_dir(&self.current_dir) {
            let mut dirs: Vec<DirEntry> = read_dir
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().map(|ft| ft.is_dir()).unwrap_or(false)
                        && !e.file_name().to_string_lossy().starts_with('.')
                })
                .map(|e| DirEntry {
                    name: e.file_name().to_string_lossy().into_owned(),
                    path: e.path(),
                    is_dir: true,
                })
                .collect();
            dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
            self.entries = dirs;
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }

    pub fn enter_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.is_dir {
                self.current_dir = entry.path.clone();
                self.path_input = self.current_dir.display().to_string();
                self.refresh_entries();
            }
        }
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            let old_name = self
                .current_dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned());
            self.current_dir = parent.to_path_buf();
            self.path_input = self.current_dir.display().to_string();
            self.refresh_entries();
            if let Some(name) = old_name {
                if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                    self.selected = idx;
                }
            }
        }
    }

    pub fn navigate_to(&mut self, path: &Path) {
        let resolved = if path.starts_with("~") {
            if let Some(home) = home_dir() {
                home.join(path.strip_prefix("~").unwrap_or(path))
            } else {
                path.to_path_buf()
            }
        } else {
            path.to_path_buf()
        };

        if resolved.is_dir() {
            self.current_dir = resolved;
            self.path_input = self.current_dir.display().to_string();
            self.refresh_entries();
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let max = self.entries.len().saturating_sub(1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, max) as usize;
    }

    pub fn sync_path_input_to_selected(&mut self) {
        self.path_input = self.selected_path().display().to_string();
    }

    pub fn apply_path_input(&mut self) {
        let expanded = if self.path_input.starts_with('~') {
            if let Some(home) = home_dir() {
                home.join(self.path_input.strip_prefix("~/").unwrap_or(
                    self.path_input.strip_prefix('~').unwrap_or(&self.path_input),
                ))
                .display()
                .to_string()
            } else {
                self.path_input.clone()
            }
        } else {
            self.path_input.clone()
        };
        let path = PathBuf::from(&expanded);
        if path.is_dir() {
            self.current_dir = path;
            self.refresh_entries();
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_test_tree() -> TempDir {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("Music/Albums")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Music/Playlists")).unwrap();
        std::fs::create_dir_all(tmp.path().join("Documents")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".hidden")).unwrap();
        tmp
    }

    #[test]
    fn lists_visible_directories_only() {
        let tmp = make_test_tree();
        let browser = FileBrowser::new(tmp.path());
        let names: Vec<&str> = browser.entries().iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Music"));
        assert!(names.contains(&"Documents"));
        assert!(!names.contains(&".hidden"));
    }

    #[test]
    fn enter_descends_into_directory() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        let music_idx = browser.entries().iter().position(|e| e.name == "Music").unwrap();
        browser.selected = music_idx;
        browser.enter_selected();
        assert_eq!(browser.current_dir(), tmp.path().join("Music"));
        let names: Vec<&str> = browser.entries().iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Albums"));
        assert!(names.contains(&"Playlists"));
    }

    #[test]
    fn go_up_returns_to_parent_and_reselects() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        let music_idx = browser.entries().iter().position(|e| e.name == "Music").unwrap();
        browser.selected = music_idx;
        browser.enter_selected();
        browser.go_up();
        assert_eq!(browser.current_dir(), tmp.path());
        assert_eq!(browser.entries()[browser.selected].name, "Music");
    }

    #[test]
    fn move_selection_clamps() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        browser.move_selection(-100);
        assert_eq!(browser.selected, 0);
        browser.move_selection(100);
        assert_eq!(browser.selected, browser.entries().len().saturating_sub(1));
    }

    #[test]
    fn navigate_to_valid_path() {
        let tmp = make_test_tree();
        let mut browser = FileBrowser::new(tmp.path());
        browser.navigate_to(&tmp.path().join("Music"));
        assert_eq!(browser.current_dir(), tmp.path().join("Music"));
    }
}
