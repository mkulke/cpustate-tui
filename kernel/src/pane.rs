//! Pane traits for scrolling and searching

use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Minimum characters before search activates
pub const MIN_SEARCH_LEN: usize = 2;

#[derive(Default)]
pub struct ScrollHints {
    pub max_offset: u16,
    pub y_offset: u16,
    pub page_height: u16,
}

pub enum ScrollDirection {
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
}

impl ScrollHints {
    /// Update scroll limits after rendering content
    pub fn update_from_render(&mut self, n_lines: usize, area_height: u16) {
        self.max_offset = (n_lines as u16).saturating_sub(area_height);
        self.page_height = area_height.saturating_sub(2);
    }

    /// Scroll in the given direction
    pub fn scroll(&mut self, direction: ScrollDirection) {
        match direction {
            ScrollDirection::Up => {
                self.y_offset = self.y_offset.saturating_sub(1);
            }
            ScrollDirection::Down => {
                if self.y_offset < self.max_offset {
                    self.y_offset = self.y_offset.saturating_add(1);
                }
            }
            ScrollDirection::Top => {
                self.y_offset = 0;
            }
            ScrollDirection::Bottom => {
                self.y_offset = self.max_offset;
            }
            ScrollDirection::PageUp => {
                self.y_offset = self.y_offset.saturating_sub(self.page_height);
            }
            ScrollDirection::PageDown => {
                self.y_offset = (self.y_offset + self.page_height).min(self.max_offset);
            }
        }
    }

    /// Scroll to a specific offset, clamped to max_offset
    pub fn scroll_to(&mut self, offset: u16) {
        self.y_offset = offset.min(self.max_offset);
    }
}

/// Trait for panes with scrollable content
pub trait Scrollable {
    fn scroll_hints_mut(&mut self) -> &mut ScrollHints;

    fn scroll(&mut self, direction: ScrollDirection) {
        self.scroll_hints_mut().scroll(direction);
    }

    fn scroll_to(&mut self, offset: u16) {
        self.scroll_hints_mut().scroll_to(offset);
    }
}

/// Trait for panes that support search - implies scrollable content
pub trait Searchable: Scrollable {
    /// Access to the pane's search state
    fn search_state_mut(&mut self) -> &mut search::SearchState;

    /// Returns searchable item names with their line offsets
    fn search_items(&self) -> Vec<(&str, u16)>;

    /// Clear search state
    fn clear_search(&mut self) {
        self.search_state_mut().clear();
    }

    /// Perform search, updating matches and scrolling to first match
    fn perform_search(&mut self, query: &str) {
        // Collect matching lines first (before mutable borrow)
        let matches: Vec<u16> = self
            .search_items()
            .into_iter()
            .filter(|(name, _)| search::smart_contains(name, query))
            .map(|(_, line)| line)
            .collect();

        let first_match = matches.first().copied();

        let search_state = self.search_state_mut();
        search_state.matches = matches;
        search_state.current_match = 0;
        search_state.last_query = query.into();

        // Jump to first match if any
        if let Some(first) = first_match {
            self.scroll_to(first);
        }
    }

    /// Navigate to next search match
    fn next_match(&mut self) {
        if let Some(offset) = self.search_state_mut().next_match() {
            self.scroll_to(offset);
        }
    }

    /// Navigate to previous search match
    fn prev_match(&mut self) {
        if let Some(offset) = self.search_state_mut().prev_match() {
            self.scroll_to(offset);
        }
    }
}

/// Create a line with optional search highlighting.
/// `name` is the searchable text, `suffix` is appended after, `name_width` pads the name.
pub fn highlight_line(
    name: &str,
    suffix: &str,
    name_width: usize,
    query: Option<&str>,
) -> Line<'static> {
    let padded_name = format!("{:<width$}", name, width = name_width);

    let Some(query) = query else {
        return Line::raw(format!("{}{}", padded_name, suffix));
    };

    if query.len() < MIN_SEARCH_LEN || !search::smart_contains(name, query) {
        return Line::raw(format!("{}{}", padded_name, suffix));
    }

    let pos = if search::has_uppercase(query) {
        name.find(query)
    } else {
        name.to_lowercase().find(&query.to_lowercase())
    };

    let Some(pos) = pos else {
        return Line::raw(format!("{}{}", padded_name, suffix));
    };

    let before = &name[..pos];
    let matched = &name[pos..pos + query.len()];
    let after = &name[pos + query.len()..];

    // Pad after match to maintain width
    let padding = name_width.saturating_sub(name.len());
    let padded_after = format!("{}{}", after, " ".repeat(padding));

    Line::from(vec![
        Span::raw(before.to_string()),
        Span::styled(
            matched.to_string(),
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
        Span::raw(padded_after),
        Span::raw(suffix.to_string()),
    ])
}
