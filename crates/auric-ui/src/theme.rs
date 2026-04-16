use crate::{Theme, ThemeStore, UiError};
use ratatui::style::Color;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Palette {
    pub use_terminal_bg: bool,
    pub surface_0: Color,
    pub surface_1: Color,
    pub surface_2: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_2: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
    pub border: Color,
    pub focus: Color,
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub selection_bg: Color,
    pub progress_fill: Color,
    pub visualizer_low: Color,
    pub visualizer_mid: Color,
    pub visualizer_high: Color,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            use_terminal_bg: true,
            surface_0: color_from_hex("#0f1115").unwrap_or(Color::Black),
            surface_1: color_from_hex("#171a21").unwrap_or(Color::Black),
            surface_2: color_from_hex("#202532").unwrap_or(Color::DarkGray),
            text: color_from_hex("#e8ecf3").unwrap_or(Color::White),
            text_muted: color_from_hex("#9aa5b5").unwrap_or(Color::Gray),
            accent: color_from_hex("#4fd1c5").unwrap_or(Color::Cyan),
            accent_2: color_from_hex("#f6ad55").unwrap_or(Color::Yellow),
            danger: color_from_hex("#fc8181").unwrap_or(Color::Red),
            warning: color_from_hex("#f6e05e").unwrap_or(Color::Yellow),
            success: color_from_hex("#68d391").unwrap_or(Color::Green),
            border: color_from_hex("#314056").unwrap_or(Color::DarkGray),
            focus: color_from_hex("#90cdf4").unwrap_or(Color::Blue),
            border_focused: color_from_hex("#90cdf4").unwrap_or(Color::Blue),
            border_unfocused: color_from_hex("#1e2736").unwrap_or(Color::DarkGray),
            selection_bg: color_from_hex("#2a3446").unwrap_or(Color::DarkGray),
            progress_fill: color_from_hex("#4fd1c5").unwrap_or(Color::Cyan),
            visualizer_low: color_from_hex("#63b3ed").unwrap_or(Color::Blue),
            visualizer_mid: color_from_hex("#4fd1c5").unwrap_or(Color::Cyan),
            visualizer_high: color_from_hex("#f6ad55").unwrap_or(Color::Yellow),
        }
    }
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let mut palette = Self::default();
        let get = |key: &str| theme.tokens.get(key).and_then(|v| color_from_hex(v));

        let mappings: [(&str, &mut Color); 19] = [
            ("colors.surface_0", &mut palette.surface_0),
            ("colors.surface_1", &mut palette.surface_1),
            ("colors.surface_2", &mut palette.surface_2),
            ("colors.text", &mut palette.text),
            ("colors.text_muted", &mut palette.text_muted),
            ("colors.accent", &mut palette.accent),
            ("colors.accent_2", &mut palette.accent_2),
            ("colors.danger", &mut palette.danger),
            ("colors.warning", &mut palette.warning),
            ("colors.success", &mut palette.success),
            ("colors.border", &mut palette.border),
            ("colors.focus", &mut palette.focus),
            ("colors.border_focused", &mut palette.border_focused),
            ("colors.border_unfocused", &mut palette.border_unfocused),
            ("colors.selection_bg", &mut palette.selection_bg),
            ("colors.progress_fill", &mut palette.progress_fill),
            ("colors.visualizer_low", &mut palette.visualizer_low),
            ("colors.visualizer_mid", &mut palette.visualizer_mid),
            ("colors.visualizer_high", &mut palette.visualizer_high),
        ];

        for (key, field) in mappings {
            if let Some(v) = get(key) {
                *field = v;
            }
        }

        palette
    }

    pub fn bg_root(&self) -> Color {
        if self.use_terminal_bg { Color::Reset } else { self.surface_0 }
    }

    pub fn bg_panel(&self) -> Color {
        if self.use_terminal_bg { Color::Reset } else { self.surface_1 }
    }
}

#[derive(Debug, Clone)]
pub struct FsThemeStore {
    base_dir: PathBuf,
}

impl FsThemeStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn load_palette(&self, name: &str) -> Result<Palette, UiError> {
        let theme = self.load(name)?;
        Ok(Palette::from_theme(&theme))
    }

    fn path_for(&self, name: &str) -> Result<PathBuf, UiError> {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains("..")
        {
            return Err(UiError::Theme(format!("invalid theme name: {name}")));
        }
        Ok(self.base_dir.join(format!("{name}.toml")))
    }
}

impl ThemeStore for FsThemeStore {
    fn load(&self, name: &str) -> Result<Theme, UiError> {
        let path = self.path_for(name)?;
        let raw = fs::read_to_string(&path)
            .map_err(|e| UiError::Theme(format!("failed to read {}: {e}", path.display())))?;
        let value: toml::Value = toml::from_str(&raw)
            .map_err(|e| UiError::Theme(format!("failed to parse {}: {e}", path.display())))?;

        let mut tokens = BTreeMap::new();
        flatten_toml("", &value, &mut tokens);

        let theme_name = value
            .get("name")
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string());

        Ok(Theme {
            name: theme_name,
            tokens,
        })
    }

    fn list(&self) -> Result<Vec<String>, UiError> {
        let mut names = Vec::new();
        for entry in fs::read_dir(&self.base_dir).map_err(|e| {
            UiError::Theme(format!("failed to read {}: {e}", self.base_dir.display()))
        })? {
            let entry = entry.map_err(|e| UiError::Theme(format!("read_dir error: {e}")))?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                names.push(stem.to_string());
            }
        }
        names.sort();
        Ok(names)
    }
}

fn flatten_toml(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let key = if prefix.is_empty() {
                    k.to_string()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten_toml(&key, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        toml::Value::Integer(i) => {
            out.insert(prefix.to_string(), i.to_string());
        }
        toml::Value::Float(f) => {
            out.insert(prefix.to_string(), f.to_string());
        }
        toml::Value::Boolean(b) => {
            out.insert(prefix.to_string(), b.to_string());
        }
        toml::Value::Datetime(dt) => {
            out.insert(prefix.to_string(), dt.to_string());
        }
        toml::Value::Array(arr) => {
            out.insert(prefix.to_string(), format!("{:?}", arr));
        }
    }
}

fn color_from_hex(input: &str) -> Option<Color> {
    let s = input.trim();
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn loads_theme_and_palette() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("demo.toml");
        fs::write(
            &path,
            "name = \"demo\"\n[colors]\ntext = \"#ffffff\"\naccent = \"#112233\"\n[layout]\npadding_x = 1\n",
        )
        .unwrap();

        let store = FsThemeStore::new(dir.path());
        let theme = store.load("demo").unwrap();
        assert_eq!(theme.name, "demo");
        assert_eq!(
            theme.tokens.get("layout.padding_x").map(String::as_str),
            Some("1")
        );

        let palette = store.load_palette("demo").unwrap();
        assert_eq!(palette.text, Color::Rgb(255, 255, 255));
        assert_eq!(palette.accent, Color::Rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn rejects_theme_name_with_path_traversal() {
        let dir = tempdir().unwrap();
        let store = FsThemeStore::new(dir.path());
        assert!(store.load("../../etc/something").is_err());
        assert!(store.load("foo/bar").is_err());
        assert!(store.load("").is_err());
    }
}
