#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseMode {
    Songs,
    Artists,
    Albums,
}

impl BrowseMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Songs => "Songs",
            Self::Artists => "Artists",
            Self::Albums => "Albums",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Songs, Self::Artists, Self::Albums]
    }
}

#[derive(Debug, Clone)]
pub struct BrowseState {
    pub mode: BrowseMode,
    pub mode_index: usize,
    pub items: Vec<String>,
    pub item_index: usize,
    pub item_scroll: usize,
    pub selected_item: Option<String>,
    pub show_items: bool,
}

impl Default for BrowseState {
    fn default() -> Self {
        Self {
            mode: BrowseMode::Songs,
            mode_index: 0,
            items: Vec::new(),
            item_index: 0,
            item_scroll: 0,
            selected_item: None,
            show_items: false,
        }
    }
}

impl BrowseState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_mode(&mut self, mode: BrowseMode) {
        self.mode = mode;
        self.mode_index = BrowseMode::all()
            .iter()
            .position(|&m| m == mode)
            .unwrap_or(0);
        self.item_index = 0;
        self.item_scroll = 0;
        self.selected_item = None;
        self.show_items = !matches!(mode, BrowseMode::Songs);
    }

    pub fn set_items(&mut self, items: Vec<String>) {
        self.items = items;
        self.item_index = 0;
        self.item_scroll = 0;
        self.selected_item = None;
    }

    pub fn move_mode_selection(&mut self, delta: isize) {
        let len = BrowseMode::all().len();
        if len == 0 {
            return;
        }
        let new_idx = if delta < 0 {
            self.mode_index
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.mode_index
                .saturating_add(delta as usize)
                .min(len.saturating_sub(1))
        };
        self.mode_index = new_idx;
        self.mode = BrowseMode::all()[self.mode_index];
    }

    pub fn move_item_selection(&mut self, delta: isize) {
        let len = self.items.len();
        if len == 0 {
            return;
        }
        let new_idx = if delta < 0 {
            self.item_index
                .saturating_sub(delta.unsigned_abs())
        } else {
            self.item_index
                .saturating_add(delta as usize)
                .min(len.saturating_sub(1))
        };
        self.item_index = new_idx;
    }

    pub fn update_selected_item(&mut self) {
        self.selected_item = self.items.get(self.item_index).cloned();
    }
}
