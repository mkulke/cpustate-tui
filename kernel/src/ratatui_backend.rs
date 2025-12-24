use alloc::string::String;
use core::convert::Infallible;
use core::fmt::{self, Write};

use ratatui::{
    backend::{Backend, ClearType, WindowSize},
    buffer::{Buffer, Cell},
    layout::{Position, Rect, Size},
    style::{Color, Modifier, Style},
};

pub struct SerialAnsiBackend<W: Write> {
    out: W,
    area: Rect,
    prev: Buffer,
    // If true, next draw will clear screen + repaint everything
    force_full_repaint: bool,
    cursor_pos: Position,
}

impl<W: Write> SerialAnsiBackend<W> {
    pub fn new(out: W, width: u16, height: u16) -> Self {
        let area = Rect::new(0, 0, width, height);
        let prev = Buffer::empty(area);
        Self {
            out,
            area,
            prev,
            force_full_repaint: true,
            cursor_pos: Position { x: 0, y: 0 },
        }
    }

    fn ansi_hide_cursor(&mut self) -> fmt::Result {
        write!(self.out, "\x1b[?25l")
    }

    fn ansi_show_cursor(&mut self) -> fmt::Result {
        write!(self.out, "\x1b[?25h")
    }

    fn ansi_reset(&mut self) -> fmt::Result {
        write!(self.out, "\x1b[0m")
    }

    fn ansi_clear_screen(&mut self) -> fmt::Result {
        // Clear + home
        write!(self.out, "\x1b[2J\x1b[H")
    }

    fn ansi_goto(&mut self, x: u16, y: u16) -> fmt::Result {
        // ANSI cursor positions are 1-based: row;col
        write!(self.out, "\x1b[{};{}H", y + 1, x + 1)
    }

    fn write_cell_symbol(&mut self, cell: &Cell) -> fmt::Result {
        // Ratatui cells carry symbols as &str (can be UTF-8, multi-width)
        // If empty, print space.
        let s = cell.symbol();
        if s.is_empty() {
            self.out.write_char(' ')
        } else {
            self.out.write_str(s)
        }
    }

    fn sgr_for_style(style: Style) -> String {
        // Build a compact SGR string like: "\x1b[0;1;38;2;R;G;B;48;5;N m"
        // We use "0" reset each time for correctness/simplicity.
        // If you want fewer bytes, you can do incremental SGR diffs later.
        let mut params: alloc::vec::Vec<u16> = alloc::vec::Vec::new();
        params.push(0);

        let add_mod = |params: &mut alloc::vec::Vec<u16>, m: Modifier| {
            if style.add_modifier.contains(m) {
                params.push(match m {
                    Modifier::BOLD => 1,
                    Modifier::DIM => 2,
                    Modifier::ITALIC => 3,
                    Modifier::UNDERLINED => 4,
                    Modifier::REVERSED => 7,
                    Modifier::CROSSED_OUT => 9,
                    _ => return,
                });
            }
        };

        add_mod(&mut params, Modifier::BOLD);
        add_mod(&mut params, Modifier::DIM);
        add_mod(&mut params, Modifier::ITALIC);
        add_mod(&mut params, Modifier::UNDERLINED);
        add_mod(&mut params, Modifier::REVERSED);
        add_mod(&mut params, Modifier::CROSSED_OUT);

        fn push_fg(params: &mut alloc::vec::Vec<u16>, c: Color) {
            match c {
                Color::Reset => {}
                Color::Black => params.push(30),
                Color::Red => params.push(31),
                Color::Green => params.push(32),
                Color::Yellow => params.push(33),
                Color::Blue => params.push(34),
                Color::Magenta => params.push(35),
                Color::Cyan => params.push(36),
                Color::Gray => params.push(37),
                Color::DarkGray => params.push(90),
                Color::LightRed => params.push(91),
                Color::LightGreen => params.push(92),
                Color::LightYellow => params.push(93),
                Color::LightBlue => params.push(94),
                Color::LightMagenta => params.push(95),
                Color::LightCyan => params.push(96),
                Color::White => params.push(97),
                Color::Indexed(n) => {
                    params.extend_from_slice(&[38, 5, n as u16]);
                }
                Color::Rgb(r, g, b) => {
                    params.extend_from_slice(&[38, 2, r as u16, g as u16, b as u16]);
                }
            }
        }

        fn push_bg(params: &mut alloc::vec::Vec<u16>, c: Color) {
            match c {
                Color::Reset => {}
                Color::Black => params.push(40),
                Color::Red => params.push(41),
                Color::Green => params.push(42),
                Color::Yellow => params.push(43),
                Color::Blue => params.push(44),
                Color::Magenta => params.push(45),
                Color::Cyan => params.push(46),
                Color::Gray => params.push(47),
                Color::DarkGray => params.push(100),
                Color::LightRed => params.push(101),
                Color::LightGreen => params.push(102),
                Color::LightYellow => params.push(103),
                Color::LightBlue => params.push(104),
                Color::LightMagenta => params.push(105),
                Color::LightCyan => params.push(106),
                Color::White => params.push(107),
                Color::Indexed(n) => {
                    params.extend_from_slice(&[48, 5, n as u16]);
                }
                Color::Rgb(r, g, b) => {
                    params.extend_from_slice(&[48, 2, r as u16, g as u16, b as u16]);
                }
            }
        }

        push_fg(&mut params, style.fg.unwrap_or(Color::Reset));
        push_bg(&mut params, style.bg.unwrap_or(Color::Reset));

        // Turn params into "\x1b[...m"
        let mut s = String::new();
        s.push_str("\x1b[");
        for (i, p) in params.iter().enumerate() {
            if i != 0 {
                s.push(';');
            }
            // small decimal append
            use alloc::fmt::format;
            s.push_str(&format(format_args!("{p}")));
        }
        s.push('m');
        s
    }

    fn paint_full(&mut self, next: &Buffer) -> fmt::Result {
        self.ansi_hide_cursor()?;
        self.ansi_clear_screen()?;

        let mut last_style = Style::default();
        // Start with a reset so last_style tracking is consistent
        self.ansi_reset()?;

        for y in 0..self.area.height {
            self.ansi_goto(0, y)?;
            for x in 0..self.area.width {
                let cell = &next[(x, y)];
                if cell.style() != last_style {
                    let sgr = Self::sgr_for_style(cell.style());
                    self.out.write_str(&sgr)?;
                    last_style = cell.style();
                }
                self.write_cell_symbol(cell)?;
            }
        }

        self.ansi_reset()?;
        self.ansi_show_cursor()?;
        Ok(())
    }

    fn paint_diff(&mut self, next: &Buffer) -> fmt::Result {
        self.ansi_hide_cursor()?;

        let mut last_style = Style::default();
        self.ansi_reset()?;

        for y in 0..self.area.height {
            for x in 0..self.area.width {
                let new = &next[(x, y)];
                let old = &self.prev[(x, y)];

                // Consider style + symbol differences
                if new.symbol() != old.symbol() || new.style() != old.style() {
                    self.ansi_goto(x, y)?;
                    if new.style() != last_style {
                        let sgr = Self::sgr_for_style(new.style());
                        self.out.write_str(&sgr)?;
                        last_style = new.style();
                    }
                    self.write_cell_symbol(new)?;
                }
            }
        }

        self.ansi_reset()?;
        self.ansi_show_cursor()?;
        Ok(())
    }
}

impl<W: Write> Backend for SerialAnsiBackend<W> {
    type Error = Infallible;

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        self.ansi_hide_cursor().unwrap();
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        self.ansi_show_cursor().unwrap();
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // serial has nothing to flush (fmt::Write), so no-op
        Ok(())
    }

    fn clear_region(&mut self, _clear_type: ClearType) -> Result<(), Self::Error> {
        self.force_full_repaint = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        // We can't query cursor position from a fmt::Write-only serial stream,
        // so we return our tracked position.
        Ok(self.cursor_pos)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let pos = position.into();
        self.cursor_pos = pos;

        // Move terminal cursor (best-effort).
        // Position in ratatui is 0-based; ANSI expects 1-based.
        self.ansi_goto(pos.x, pos.y).unwrap();
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        // Clear the physical terminal screen and reset our "previous frame"
        // so the next draw doesn't try to diff against stale content.
        self.ansi_hide_cursor().unwrap();
        self.ansi_clear_screen().unwrap();
        self.ansi_reset().unwrap();

        self.prev = Buffer::empty(self.area);
        self.force_full_repaint = false;

        // Cursor ends up at home after clear+home.
        self.cursor_pos = Position { x: 0, y: 0 };
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(Size {
            width: self.area.width,
            height: self.area.height,
        })
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        // We don't have a way to query pixel size over serial; report only cell size.
        // Many consumers only care about `columns_rows`.
        let columns_rows = Size {
            width: self.area.width,
            height: self.area.height,
        };
        Ok(WindowSize {
            columns_rows,
            pixels: columns_rows,
        })
    }

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        // Apply incoming cell updates into a "next" buffer
        let mut next = self.prev.clone();
        for (x, y, cell) in content {
            if x < self.area.width && y < self.area.height {
                next[(x, y)] = cell.clone();
            }
        }

        if self.force_full_repaint {
            let _ = self.paint_full(&next);
            self.force_full_repaint = false;
        } else {
            let _ = self.paint_diff(&next);
        }

        self.prev = next;
        Ok(())
    }
}
