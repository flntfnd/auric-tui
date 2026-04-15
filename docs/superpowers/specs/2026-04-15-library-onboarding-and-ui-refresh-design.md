# Library Onboarding and UI Refresh

## Overview

Two related changes to the auric TUI: a library onboarding flow that helps new users add music folders, and a layout refresh that improves whitespace, readability, and visual hierarchy across the entire shell.

The layout refresh ships first. The onboarding flow builds on top of it.

---

## Part 1: UI Layout Refresh

### Problem

The current layout packs content tight against borders with no inner padding, uses a dense status bar, and lacks visual hierarchy between sections. It reads like a debug dump rather than an application.

### Design Principles

Drawn from the reference TUIs (Superfile, Framework system tool, ledger TUI):

- **Inner padding**: 1-cell horizontal padding inside every panel. Content never touches the border.
- **Section headers**: Panel titles rendered with spacing, not crammed into the border line.
- **Breathing room**: 1-cell vertical gaps between stacked panels where terminal height allows.
- **Calm status**: The bottom status bar shows only the essentials. Move verbose info behind `?` help.
- **Visual hierarchy through spacing**: Use whitespace to group related content rather than relying solely on color.

### Changes to `crates/auric-ui/src/shell.rs`

**Layout constants** (new):
- `PANEL_INNER_PADDING_H: u16 = 1` -- horizontal padding inside panels
- `PANEL_INNER_PADDING_V: u16 = 0` -- vertical padding (0 for now, panels are already tight on height)
- `PANEL_GAP: u16 = 1` -- gap between adjacent panels

**Panel rendering**:
- Every `Block::bordered()` call gets an inner margin applied via `Margin { horizontal: 1, vertical: 0 }` on the inner area before rendering content.
- Panel titles use `Title::from()` with a leading space for visual offset from the border corner.

**Left sidebar** (roots, browse library, playlists):
- Add 1-cell gap between the three stacked sections.
- "Matched Directories" label replaced with "Library Roots" for clarity.

**Tracks pane**:
- Column headers get 1 additional row of spacing below them before the first track row.
- Track rows use consistent column alignment with breathing room between columns.

**Status bar**:
- Reduce to a single line: focus pane indicator, track count, current filter (if active).
- Move mode, theme, db path, icon pack info behind `?` help overlay.

**Now Playing panel**:
- Remains top-right. Add inner padding to match other panels.

### Files touched

- `crates/auric-ui/src/shell.rs` -- layout calculations, render functions, status bar

---

## Part 2: Library Onboarding Flow

### Components

Three features, all in service of the same goal: help users get music into the library.

#### 2a. First-Run Welcome Panel

**Trigger**: On TUI launch, if `snapshot.roots.is_empty() && snapshot.tracks.is_empty()`, display the welcome variant of the Add Music panel automatically.

**Behavior**:
- Centered floating panel with welcome header: "Welcome to auric"
- Subtitle: "Add a folder to get started."
- Contains the same tree browser + path input as the standard Add Music panel (see 2c).
- `Esc` dismisses to the empty TUI (does not force the user to add a folder).
- After adding a folder, the panel closes, a scan runs, and the library populates.

**State**: New variant `InputMode::Welcome` that renders the Add Music panel with the welcome header. On dismiss, transitions to `InputMode::Normal`.

#### 2b. Empty State Hints

When the welcome panel is dismissed (or after the first run flag is no longer relevant), the empty panes show contextual hints instead of bare "No X" messages.

**Library Roots pane** (when empty):
```
No library roots

  Press a to add a music folder
```

**Tracks pane** (when empty, and roots also empty):
```
No tracks in library

  Add a music folder to get started
  Press a or : then root add /path
```

**Tracks pane** (when empty, but roots exist):
```
No tracks in library

  Press : then scan roots to import
```

Hints render in the `DimmedText` style from the palette so they don't compete with primary content.

#### 2c. Add Music Floating Panel

**Trigger**: `a` key in Normal mode (any pane). Also triggered by first-run welcome flow.

**State**: New variant `InputMode::AddMusic`.

**Panel layout** (centered, 60% terminal width, 70% terminal height, min 40x16):

```
┌─ Add Music ──────────────────────────────────┐
│                                               │
│  Path: /Users/rob/Music▌                      │
│                                               │
│  ~ /                                          │
│    .config/                                   │
│    Desktop/                                   │
│    Documents/                                 │
│    Downloads/                                 │
│   ▸ Music/ ◀                                  │
│    Pictures/                                  │
│    Projects/                                  │
│                                               │
│  enter select  esc cancel  tab toggle input   │
│  drag folders here to add                     │
└───────────────────────────────────────────────┘
```

**Two interaction modes within the panel** (toggled with `Tab`):

1. **Tree browser** (default focus): Arrow keys navigate the directory tree. `Enter` descends into the highlighted directory. `Backspace` or `h` goes up one level. The path input updates to reflect the current directory. To confirm the current directory as a library root, press `Enter` on the `[Add this folder]` action row at the top of the listing (always present, styled distinctly). The path input updates to reflect the currently highlighted directory.

2. **Path input**: Free-text path entry. As the user types, the tree browser navigates to match. `Enter` confirms the path. Supports `~` expansion.

**After selection**:
- The selected path is added as a library root via `db.upsert_library_root()`.
- An automatic scan runs on the new root.
- The panel closes and the snapshot refreshes.
- Status message confirms: "Added /Users/rob/Music (imported 847 tracks)"

**Multiple folders**: After adding one folder, the panel remains open so the user can add more. A "Done" hint appears alongside the keybindings. `Esc` closes.

**Tree browser implementation**:
- New struct `FileBrowser` in `crates/auric-ui/src/file_browser.rs`.
- Holds: `current_dir: PathBuf`, `entries: Vec<DirEntry>`, `selected_index: usize`, `scroll_offset: usize`.
- Methods: `navigate_to(path)`, `enter_selected()`, `go_up()`, `refresh_entries()`.
- Only shows directories (not files) since we're selecting library roots.
- Entries sorted: directories first, then alphabetical. Hidden dirs (`.` prefix) excluded by default.
- Starts at the user's home directory.

#### 2d. Drag and Drop Support

**Terminal detection**: At TUI startup, check for drag-and-drop support by inspecting the `$TERM_PROGRAM` environment variable and known-supported terminals:
- iTerm2 (`iTerm.app` or `iTerm2`)
- Kitty (`kitty`)
- WezTerm (`WezTerm`)
- Ghostty (`ghostty`)
- foot (`foot`)

**Implementation**: Terminals that support file drop emit an OSC 52 or bracketed paste sequence containing the file path. crossterm's `Event::Paste(String)` captures bracketed paste content. For drag-and-drop:
- Enable bracketed paste mode alongside mouse capture in the terminal setup.
- When `Event::Paste` is received and `InputMode` is `Normal` or `AddMusic`:
  - Parse the pasted content as a path (or newline-separated paths for multiple).
  - Validate each path exists and is a directory.
  - If in `AddMusic` mode, navigate the tree browser to that path and auto-select.
  - If in `Normal` mode, add directly as a library root and scan.
  - Status message confirms what was added.

**Hint rendering**: The "drag folders here to add" hint in the Add Music panel only renders when a supported terminal is detected. Otherwise, that line is omitted.

### New files

- `crates/auric-ui/src/file_browser.rs` -- `FileBrowser` struct and directory navigation logic

### Modified files

- `crates/auric-ui/src/shell.rs` -- new `InputMode` variants, keybinding for `a`, panel rendering, empty state hints, drag-and-drop event handling, terminal detection
- `crates/auric-app/src/lib.rs` -- wire up the add-root-and-scan action into the command handler callback, first-run detection logic

---

## Implementation Order

1. **UI layout refresh** -- padding, gaps, status bar cleanup (Part 1)
2. **File browser module** -- standalone `FileBrowser` struct with directory navigation (Part 2c, just the data model)
3. **Add Music panel** -- floating panel rendering, `InputMode::AddMusic`, `a` keybinding, tree browser + path input (Part 2c)
4. **First-run welcome** -- `InputMode::Welcome` variant, auto-trigger on empty library (Part 2a)
5. **Empty state hints** -- contextual messages in empty panes (Part 2b)
6. **Drag and drop** -- terminal detection, paste event handling (Part 2d)

Steps 1-3 are sequential. Steps 4, 5, and 6 are independent of each other and can be parallelized after step 3.

---

## Testing

- **FileBrowser**: Unit tests with a temp directory tree. Test navigation, go_up, hidden dir filtering, home expansion.
- **Empty state hints**: Snapshot test with empty roots/tracks confirming hint text appears.
- **Terminal detection**: Unit test mapping `TERM_PROGRAM` values to support flags.
- **Integration**: Manual testing of the full flow -- launch with empty db, see welcome panel, browse to a folder, add it, confirm tracks appear. Dismiss and re-add via `a` key. Drag and drop in a supported terminal.
