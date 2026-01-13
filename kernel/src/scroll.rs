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
