use std::collections::BTreeMap;

pub mod file_browser;
pub mod shell;
pub mod terminal_caps;
pub mod theme;

pub use shell::{
    render_once_to_text, run_interactive, run_interactive_with_handlers,
    run_interactive_with_refresh, run_interactive_with_scan, FocusPane, IconMode,
    PaletteCommandResult, RunOptions, ScanProgress, ShellListItem, ShellSnapshot, ShellState,
    ShellTrackItem,
};
pub use theme::{FsThemeStore, Palette};

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub tokens: BTreeMap<String, String>,
}

pub trait ThemeStore: Send + Sync {
    fn load(&self, name: &str) -> Result<Theme, UiError>;
    fn list(&self) -> Result<Vec<String>, UiError>;
}

#[derive(Debug, thiserror::Error)]
pub enum UiError {
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("theme error: {0}")]
    Theme(String),
}
