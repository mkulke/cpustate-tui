use ratatui::style::Color;

pub struct AppState {
    color_idx: usize,
}

impl AppState {
    pub fn new() -> Self {
        Self { color_idx: 0 }
    }

    pub fn tick(&mut self) {
        self.color_idx = (self.color_idx + 1) % 8;
    }

    pub fn color(&self) -> Color {
        const COLORS: [Color; 8] = [
            Color::Magenta,
            Color::Red,
            Color::Yellow,
            Color::Blue,
            Color::White,
            Color::Cyan,
            Color::Green,
            Color::DarkGray,
        ];
        COLORS[self.color_idx]
    }
}
