use std::collections::BTreeMap;

pub mod shell;
pub mod theme;

pub use shell::{
    render_once_to_text, run_interactive, run_interactive_with_handlers,
    run_interactive_with_refresh, FocusPane, IconMode, PaletteCommandResult, RunOptions,
    ShellListItem, ShellSnapshot, ShellState, ShellTrackItem,
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
