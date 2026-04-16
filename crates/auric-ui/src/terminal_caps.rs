use std::env;

#[derive(Debug, Clone)]
pub struct TerminalCaps {
    pub supports_drag_drop: bool,
    pub terminal_name: String,
}

impl TerminalCaps {
    pub fn detect() -> Self {
        let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
        let supports_drag_drop = matches!(
            term_program.as_str(),
            "iTerm.app" | "iTerm2" | "WezTerm" | "ghostty" | "foot"
        ) || env::var("TERM").is_ok_and(|t| t.contains("kitty"));

        Self {
            supports_drag_drop,
            terminal_name: term_program,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_construction() {
        let caps = TerminalCaps {
            supports_drag_drop: true,
            terminal_name: "ghostty".to_string(),
        };
        assert!(caps.supports_drag_drop);
    }
}
