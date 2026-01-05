use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Available theme presets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemePreset {
    #[default]
    Default,
    Dracula,
    Gruvbox,
}

impl ThemePreset {
    pub fn label(&self) -> &'static str {
        match self {
            ThemePreset::Default => "Default",
            ThemePreset::Dracula => "Dracula",
            ThemePreset::Gruvbox => "Gruvbox",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            ThemePreset::Default => ThemePreset::Dracula,
            ThemePreset::Dracula => ThemePreset::Gruvbox,
            ThemePreset::Gruvbox => ThemePreset::Default,
        }
    }

    pub fn theme(&self) -> Theme {
        match self {
            ThemePreset::Default => Theme::default_theme(),
            ThemePreset::Dracula => Theme::dracula(),
            ThemePreset::Gruvbox => Theme::gruvbox(),
        }
    }
}

/// Theme colors for the entire application
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Theme {
    // Base colors
    pub background: Color,
    pub foreground: Color,
    pub foreground_dim: Color,
    pub foreground_bright: Color,

    // UI elements
    pub border_active: Color,
    pub border_inactive: Color,
    pub border_highlight: Color,

    // Selection/cursor
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub cursor_bg: Color,
    pub cursor_fg: Color,

    // Accent colors for different panels
    pub accent_primary: Color,    // Main accent (folders, tracks)
    pub accent_secondary: Color,  // Secondary accent (watched folders)
    pub accent_tertiary: Color,   // Tertiary accent (playlists)

    // Status colors
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // Now playing
    pub now_playing_accent: Color,
    pub progress_bar_filled: Color,
    pub progress_bar_empty: Color,

    // Spectrum analyzer
    pub spectrum_low: Color,
    pub spectrum_mid: Color,
    pub spectrum_high: Color,

    // Track list
    pub track_playing: Color,
    pub track_selected: Color,
    pub header: Color,

    // Dialogs
    pub dialog_border: Color,
    pub dialog_title: Color,
    pub input_bg: Color,
    pub input_fg: Color,

    // Hints/help
    pub hint_key: Color,
    pub hint_text: Color,
}

impl Theme {
    /// Default cyan-focused theme
    pub fn default_theme() -> Self {
        Self {
            // Base colors
            background: Color::Reset,
            foreground: Color::White,
            foreground_dim: Color::DarkGray,
            foreground_bright: Color::White,

            // UI elements
            border_active: Color::Cyan,
            border_inactive: Color::DarkGray,
            border_highlight: Color::Yellow,

            // Selection/cursor
            selection_bg: Color::Cyan,
            selection_fg: Color::Black,
            cursor_bg: Color::Cyan,
            cursor_fg: Color::Black,

            // Accent colors
            accent_primary: Color::Cyan,
            accent_secondary: Color::Magenta,
            accent_tertiary: Color::Green,

            // Status colors
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            info: Color::Blue,

            // Now playing
            now_playing_accent: Color::Cyan,
            progress_bar_filled: Color::Cyan,
            progress_bar_empty: Color::DarkGray,

            // Spectrum analyzer
            spectrum_low: Color::Green,
            spectrum_mid: Color::Yellow,
            spectrum_high: Color::Red,

            // Track list
            track_playing: Color::Green,
            track_selected: Color::Cyan,
            header: Color::Yellow,

            // Dialogs
            dialog_border: Color::Cyan,
            dialog_title: Color::Cyan,
            input_bg: Color::DarkGray,
            input_fg: Color::White,

            // Hints/help
            hint_key: Color::Yellow,
            hint_text: Color::DarkGray,
        }
    }

    /// Dracula theme - purple/pink focused
    pub fn dracula() -> Self {
        // Dracula palette
        let background = Color::Rgb(40, 42, 54);
        let foreground = Color::Rgb(248, 248, 242);
        let comment = Color::Rgb(98, 114, 164);
        let cyan = Color::Rgb(139, 233, 253);
        let green = Color::Rgb(80, 250, 123);
        let orange = Color::Rgb(255, 184, 108);
        let pink = Color::Rgb(255, 121, 198);
        let purple = Color::Rgb(189, 147, 249);
        let red = Color::Rgb(255, 85, 85);
        let yellow = Color::Rgb(241, 250, 140);

        Self {
            // Base colors
            background,
            foreground,
            foreground_dim: comment,
            foreground_bright: foreground,

            // UI elements
            border_active: purple,
            border_inactive: comment,
            border_highlight: yellow,

            // Selection/cursor
            selection_bg: purple,
            selection_fg: background,
            cursor_bg: purple,
            cursor_fg: background,

            // Accent colors
            accent_primary: purple,
            accent_secondary: pink,
            accent_tertiary: green,

            // Status colors
            success: green,
            warning: yellow,
            error: red,
            info: cyan,

            // Now playing
            now_playing_accent: pink,
            progress_bar_filled: purple,
            progress_bar_empty: comment,

            // Spectrum analyzer
            spectrum_low: cyan,
            spectrum_mid: purple,
            spectrum_high: pink,

            // Track list
            track_playing: green,
            track_selected: purple,
            header: orange,

            // Dialogs
            dialog_border: purple,
            dialog_title: pink,
            input_bg: Color::Rgb(68, 71, 90),
            input_fg: foreground,

            // Hints/help
            hint_key: yellow,
            hint_text: comment,
        }
    }

    /// Gruvbox theme - warm retro colors
    pub fn gruvbox() -> Self {
        // Gruvbox dark palette
        let bg = Color::Rgb(40, 40, 40);
        let fg = Color::Rgb(235, 219, 178);
        let gray = Color::Rgb(146, 131, 116);
        let red = Color::Rgb(251, 73, 52);
        let green = Color::Rgb(184, 187, 38);
        let yellow = Color::Rgb(250, 189, 47);
        let blue = Color::Rgb(131, 165, 152);
        let _purple = Color::Rgb(211, 134, 155);
        let aqua = Color::Rgb(142, 192, 124);
        let orange = Color::Rgb(254, 128, 25);

        Self {
            // Base colors
            background: bg,
            foreground: fg,
            foreground_dim: gray,
            foreground_bright: fg,

            // UI elements
            border_active: yellow,
            border_inactive: gray,
            border_highlight: orange,

            // Selection/cursor
            selection_bg: yellow,
            selection_fg: bg,
            cursor_bg: yellow,
            cursor_fg: bg,

            // Accent colors
            accent_primary: yellow,
            accent_secondary: orange,
            accent_tertiary: aqua,

            // Status colors
            success: green,
            warning: yellow,
            error: red,
            info: blue,

            // Now playing
            now_playing_accent: orange,
            progress_bar_filled: yellow,
            progress_bar_empty: gray,

            // Spectrum analyzer
            spectrum_low: aqua,
            spectrum_mid: yellow,
            spectrum_high: orange,

            // Track list
            track_playing: green,
            track_selected: yellow,
            header: orange,

            // Dialogs
            dialog_border: yellow,
            dialog_title: orange,
            input_bg: Color::Rgb(60, 56, 54),
            input_fg: fg,

            // Hints/help
            hint_key: yellow,
            hint_text: gray,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}
