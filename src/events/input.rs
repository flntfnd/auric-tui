use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Navigation
    Up,
    Down,
    Left,
    Right,
    NextPanel,
    PrevPanel,
    Enter,
    Escape,

    // Playback
    PlayPause,
    Stop,
    NextTrack,
    PrevTrack,
    SeekForward,
    SeekBackward,
    VolumeUp,
    VolumeDown,

    // Modes
    ToggleShuffle,
    ToggleRepeat,
    CycleSortMode,

    // Library
    LoadFolder,
    SetWatchFolder,
    Refresh,
    FetchArtwork,

    // Playlist
    NewPlaylist,
    AddToPlaylist,
    RemoveFromPlaylist,
    DeletePlaylist,

    // UI
    Help,
    Search,
    Settings,
    Quit,

    // Special
    Confirm,
    #[allow(dead_code)]
    Cancel,
    Delete,
    Backspace,
    Char(char),

    // Mouse actions
    MouseClick { x: u16, y: u16 },
    MouseDrag { x: u16, y: u16 },
    MouseScrollUp { x: u16, y: u16 },
    MouseScrollDown { x: u16, y: u16 },

    // No action
    None,
}

impl Action {
    pub fn from_key_event(key: KeyEvent) -> Self {
        match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => Action::Quit,
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => Action::Quit,

            // Navigation
            (KeyCode::Up | KeyCode::Char('k'), KeyModifiers::NONE) => Action::Up,
            (KeyCode::Down | KeyCode::Char('j'), KeyModifiers::NONE) => Action::Down,
            (KeyCode::Left | KeyCode::Char('h'), KeyModifiers::NONE) => Action::Left,
            (KeyCode::Right | KeyCode::Char('l'), KeyModifiers::NONE) => Action::Right,
            (KeyCode::Tab, KeyModifiers::NONE) => Action::NextPanel,
            (KeyCode::BackTab, KeyModifiers::SHIFT) => Action::PrevPanel,
            (KeyCode::Enter, _) => Action::Enter,
            (KeyCode::Esc, _) => Action::Escape,

            // Playback
            (KeyCode::Char(' '), KeyModifiers::NONE) => Action::PlayPause,
            (KeyCode::Char('x'), KeyModifiers::NONE) => Action::Stop,
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => Action::NextTrack,
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => Action::PrevTrack,
            (KeyCode::Char(']'), KeyModifiers::NONE) => Action::SeekForward,
            (KeyCode::Char('['), KeyModifiers::NONE) => Action::SeekBackward,
            (KeyCode::Char('+') | KeyCode::Char('='), _) => Action::VolumeUp,
            (KeyCode::Char('-'), KeyModifiers::NONE) => Action::VolumeDown,

            // Modes
            (KeyCode::Char('s'), KeyModifiers::NONE) => Action::ToggleShuffle,
            (KeyCode::Char('r'), KeyModifiers::NONE) => Action::ToggleRepeat,
            (KeyCode::Char('S'), KeyModifiers::SHIFT) => Action::CycleSortMode,

            // Library
            (KeyCode::Char('o'), KeyModifiers::NONE) => Action::LoadFolder,
            (KeyCode::Char('w'), KeyModifiers::NONE) => Action::SetWatchFolder,
            (KeyCode::Char('R'), KeyModifiers::SHIFT) => Action::Refresh,
            (KeyCode::Char('A'), KeyModifiers::SHIFT) => Action::FetchArtwork,

            // Playlist
            (KeyCode::Char('N'), KeyModifiers::SHIFT) => Action::NewPlaylist,
            (KeyCode::Char('a'), KeyModifiers::NONE) => Action::AddToPlaylist,
            (KeyCode::Char('d'), KeyModifiers::NONE) => Action::RemoveFromPlaylist,
            (KeyCode::Char('D'), KeyModifiers::SHIFT) => Action::DeletePlaylist,

            // UI
            (KeyCode::Char('?'), KeyModifiers::NONE) => Action::Help,
            (KeyCode::Char('f'), KeyModifiers::CONTROL) => Action::Search,
            (KeyCode::Char(','), KeyModifiers::NONE) => Action::Settings,

            // Special
            (KeyCode::Char('y'), KeyModifiers::NONE) => Action::Confirm,
            (KeyCode::Delete, _) => Action::Delete,
            (KeyCode::Backspace, _) => Action::Backspace,
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => Action::Char(c),

            _ => Action::None,
        }
    }
}
