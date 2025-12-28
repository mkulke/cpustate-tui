use crate::app::{App, CpuidState, Pane};
use crate::ratatui_backend::SerialAnsiBackend;
use alloc::format;
use alloc::vec;
use core::fmt::LowerHex;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use uart_16550::SerialPort;

pub struct View<'a> {
    terminal: Terminal<SerialAnsiBackend<&'a mut SerialPort>>,
    max_scroll: Option<u16>,
}

impl<'a> View<'a> {
    pub fn new(port: &'a mut SerialPort) -> Self {
        let backend = SerialAnsiBackend::new(port, 80, 24);
        let terminal = Terminal::new(backend).unwrap();

        Self {
            terminal,
            max_scroll: None,
        }
    }

    pub fn draw(&mut self, app: &App) {
        // let terminal = &mut self.terminal;
        self.terminal
            .draw(|f| {
                let max_scroll = ui(f, &app);
                self.max_scroll = max_scroll;
            })
            .unwrap();
    }

    pub fn scroll_up(&mut self, app: &mut App) {
        app.scroll_up();
    }

    pub fn scroll_down(&mut self, app: &mut App) {
        app.scroll_down(self.max_scroll);
    }
}

fn draw_dummy_pane(rect: Rect, frame: &mut Frame, app: &App) -> Option<u16> {
    let [_, message_slot, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(rect);

    let paragraph = Paragraph::new(Line::styled(
        "Hello from Ratatui!",
        Style::default().fg(app.color()),
    ))
    .centered();

    frame.render_widget(paragraph, message_slot);

    None
}

fn draw_cpuid_content(rect: Rect, frame: &mut Frame, state: &CpuidState) -> Option<u16> {
    let vendor_info = Line::raw(format!("vendor_info=\"{}\"", state.vendor_info()));
    let features_header = Line::styled("Features", Style::default().bold());
    let mut lines = vec![vendor_info, features_header];
    for feature in state.features() {
        let line = Line::raw(format!("{}={}", feature.0, feature.1));
        lines.push(line);
    }

    let n_lines = lines.len();
    let paragraph = Paragraph::new(lines).scroll((state.y_offset(), 0));

    frame.render_widget(paragraph, rect);

    let max_scroll = (n_lines as u16) - rect.height;

    Some(max_scroll)
}

struct XmmBytes([u8; 16]);

impl LowerHex for XmmBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for &b in self.0.iter().rev() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

fn draw_fpu_content(rect: Rect, frame: &mut Frame, app: &App) -> Option<u16> {
    let area = app.fxsave64();

    let header = Line::styled("fxsave64", Style::default().bold());
    let line = Line::raw(format!("mcxsr=0x{:x}", area.0.mxcsr));
    let mut text = vec![header, line];
    for i in 0..16 {
        let value = XmmBytes(area.0.xmm[i]);
        let line = format!("xmm{:02}={:x}", i, value);
        text.push(Line::raw(line));
    }

    let paragraph = Paragraph::new(Text::from(text));
    frame.render_widget(paragraph, rect);

    None
}

fn ui(f: &mut Frame<'_>, app: &App) -> Option<u16> {
    let area = f.area();

    let pane_block = Block::default()
        .title(app.pane_title())
        .borders(Borders::ALL);

    let [pane_area, bottom_bar] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Fill(1), Constraint::Length(1)])
        .areas(area);

    let pane_inner = pane_block.inner(pane_area);

    f.render_widget(pane_block, pane_area);

    let max_scroll = match app.pane() {
        Pane::Fpu => draw_fpu_content(pane_inner, f, app),
        Pane::Cpuid => draw_cpuid_content(pane_inner, f, app.cpuid_state()),
        _ => draw_dummy_pane(pane_inner, f, app),
    };

    let caption = "CPUID (c) | FPU (f) | XSAVE (x) | Quit (q)";
    f.render_widget(caption, bottom_bar);

    max_scroll
}
