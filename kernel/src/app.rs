use alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::LowerHex;
use core::sync::atomic::Ordering;
use x86_64::instructions;

use crate::cpuid::{self, CpuFeatures, ExtendedStateFeatures, VendorInfo};
use crate::fpu::{enable_avx, enable_sse, fxsave64, read_ymm_registers, set_xmm0_bytes, set_xmm15_bytes, FxSaveAligned, YmmRegisters};
use crate::interrupts;
use crate::lapic::{lapic_timer_freq_hz, TARGET_TIMER_HZ};
use crate::qemu::{self, QemuExitCode};
use crate::ratatui_backend::SerialAnsiBackend;
use crate::serial::{self, SerialPort};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Terminal;

struct XmmBytes([u8; 16]);

impl LowerHex for XmmBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for &b in self.0.iter().rev() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

struct YmmBytes([u8; 32]);

impl LowerHex for YmmBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for &b in self.0.iter().rev() {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

pub struct CpuidState {
    features: CpuFeatures,
}

impl CpuidState {
    fn new() -> Self {
        let features = CpuFeatures::new();

        Self { features }
    }

    fn features(&self) -> &Vec<(&'static str, bool)> {
        &self.features.features()
    }

    fn extended_features(&self) -> &Vec<(&'static str, bool)> {
        &self.features.extended_features()
    }

    fn extended_state_features(&self) -> &ExtendedStateFeatures {
        &self.features.extended_state_features()
    }

    fn vendor_info(&self) -> &VendorInfo {
        self.features.vendor_info()
    }

    fn has_xsave(&self) -> bool {
        self.features.has_xsave()
    }

    fn leaf_0xd_0(&self) -> [u32; 4] {
        self.features.leaf(0xD, 0)
    }

    fn leaf_0xd_1(&self) -> [u32; 4] {
        self.features.leaf(0xD, 1)
    }

    fn leaf_0x1_0(&self) -> [u32; 4] {
        self.features.leaf(0x1, 0)
    }

    fn has_avx2(&self) -> bool {
        self.features.has_avx2()
    }
}

pub enum Pane {
    Cpuid,
    Fpu,
    Xsave,
    Timer,
}

#[derive(Default)]
struct PaneScrollHints {
    max_offset: u16,
    y_offset: u16,
    page_height: u16,
}

#[derive(Default)]
struct ScrollHints {
    cpuid: PaneScrollHints,
    fpu: PaneScrollHints,
}

pub struct App {
    color_idx: usize,
    pane: Pane,
    cpuid_state: CpuidState,
    scroll_hints: ScrollHints,
    last_g_tick: Option<usize>,
}

fn write_xmm_values() {
    let mut xmm = [0u8; 16];
    let a = 0x0011223344556677u64;
    let b = 0x8899AABBCCDDEEFFu64;
    xmm[0..8].copy_from_slice(&a.to_le_bytes());
    xmm[8..16].copy_from_slice(&b.to_le_bytes());
    set_xmm0_bytes(&xmm);
    xmm.reverse();
    set_xmm15_bytes(&xmm);
}

enum InputEvent {
    Quit,
    ScrollToTop,
    ScrollToBottom,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    SelectPane(Pane),
}

enum ScrollDirection {
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
}

/// Max ticks between two 'g' presses to trigger gg (500ms at TARGET_TIMER_HZ)
const GG_TIMEOUT_TICKS: usize = (TARGET_TIMER_HZ / 2) as usize;

impl App {
    pub fn new() -> Self {
        enable_sse();
        write_xmm_values();

        let cpuid_state = CpuidState::new();

        // Enable AVX if the CPU supports AVX2
        if cpuid_state.has_avx2() {
            enable_avx();
        }

        Self {
            color_idx: 0,
            pane: Pane::Cpuid,
            cpuid_state,
            scroll_hints: ScrollHints::default(),
            last_g_tick: None,
        }
    }

    fn tick(&mut self) {
        self.color_idx = (self.color_idx + 1) % 8;
    }

    fn color(&self) -> Color {
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

    fn scroll(&mut self, direction: ScrollDirection) {
        let (y_offset, max_offset, page_height) = match self.pane {
            Pane::Cpuid => (
                &mut self.scroll_hints.cpuid.y_offset,
                self.scroll_hints.cpuid.max_offset,
                self.scroll_hints.cpuid.page_height,
            ),
            Pane::Fpu => (
                &mut self.scroll_hints.fpu.y_offset,
                self.scroll_hints.fpu.max_offset,
                self.scroll_hints.fpu.page_height,
            ),
            _ => return,
        };

        match direction {
            ScrollDirection::Up => {
                *y_offset = y_offset.saturating_sub(1);
            }
            ScrollDirection::Down => {
                if *y_offset < max_offset {
                    *y_offset = y_offset.saturating_add(1);
                }
            }
            ScrollDirection::Top => {
                *y_offset = 0;
            }
            ScrollDirection::Bottom => {
                *y_offset = max_offset;
            }
            ScrollDirection::PageUp => {
                *y_offset = y_offset.saturating_sub(page_height);
            }
            ScrollDirection::PageDown => {
                *y_offset = (*y_offset + page_height).min(max_offset);
            }
        }
    }

    fn pane_title(&self) -> &'static str {
        match self.pane {
            Pane::Cpuid => "CPUID",
            Pane::Fpu => "FPU",
            Pane::Xsave => "XSAVE",
            Pane::Timer => "Timer",
        }
    }

    fn fxsave64(&self) -> FxSaveAligned {
        let mut area = FxSaveAligned::new_zeroed();
        fxsave64(&mut area);
        area
    }

    fn handle_input(&mut self) -> Option<InputEvent> {
        let mut event = None;

        serial::RX_QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            let (_prod, mut cons) = queue.split();

            let Some(byte) = cons.dequeue() else {
                return;
            };

            event = match byte {
                b'q' => Some(InputEvent::Quit),
                b'c' => Some(InputEvent::SelectPane(Pane::Cpuid)),
                b'f' => Some(InputEvent::SelectPane(Pane::Fpu)),
                b'x' => Some(InputEvent::SelectPane(Pane::Xsave)),
                b't' => Some(InputEvent::SelectPane(Pane::Timer)),
                b'j' => Some(InputEvent::ScrollDown),
                b'k' => Some(InputEvent::ScrollUp),
                b'G' => Some(InputEvent::ScrollToBottom),
                0x06 => Some(InputEvent::PageDown), // Ctrl+F
                0x02 => Some(InputEvent::PageUp),   // Ctrl+B
                b'g' => {
                    let now = interrupts::tick_count();
                    let Some(last) = self.last_g_tick else {
                        self.last_g_tick = Some(now);
                        return;
                    };
                    if now.saturating_sub(last) <= GG_TIMEOUT_TICKS {
                        self.last_g_tick = None;
                        Some(InputEvent::ScrollToTop)
                    } else {
                        self.last_g_tick = Some(now);
                        None
                    }
                }
                _ => None,
            };
        });

        event
    }

    fn handle_ticks(&mut self) -> bool {
        let n = interrupts::PRINT_EVENTS.swap(0, Ordering::AcqRel);
        let mut update = false;
        for _ in 0..n {
            self.tick();
            update = true;
        }
        update
    }

    fn draw(&mut self, terminal: &mut Terminal<SerialAnsiBackend<SerialPort>>) {
        terminal
            // self implements Widget
            .draw(|frame| frame.render_widget(&mut *self, frame.area()))
            .unwrap();
    }

    pub fn run(&mut self) -> ! {
        let port = serial::port();
        let backend = SerialAnsiBackend::new(port, 80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        // initial draw
        self.draw(&mut terminal);

        loop {
            instructions::hlt();

            let mut needs_redraw = self.handle_ticks();

            let event = self.handle_input();
            if let Some(event) = event {
                match event {
                    InputEvent::Quit => qemu::exit(QemuExitCode::Success),
                    InputEvent::ScrollToTop => self.scroll(ScrollDirection::Top),
                    InputEvent::ScrollToBottom => self.scroll(ScrollDirection::Bottom),
                    InputEvent::PageUp => self.scroll(ScrollDirection::PageUp),
                    InputEvent::PageDown => self.scroll(ScrollDirection::PageDown),
                    InputEvent::SelectPane(pane) => self.pane = pane,
                    InputEvent::ScrollUp => self.scroll(ScrollDirection::Up),
                    InputEvent::ScrollDown => self.scroll(ScrollDirection::Down),
                }
                needs_redraw = true;
            }

            if needs_redraw {
                self.draw(&mut terminal);
                needs_redraw = false;
            }
        }
    }

    fn render_timer_pane(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![Line::styled("Timer Calibration", Style::default().bold())];
        lines.push(Line::raw(""));

        // TSC frequency from CPUID
        let tsc_freq_str = match cpuid::tsc_frequency() {
            Some(freq) => format!("{} Hz ({:.2} GHz)", freq, freq as f64 / 1_000_000_000.0),
            None => "Not available".into(),
        };
        lines.push(Line::raw(format!("TSC Frequency:   {}", tsc_freq_str)));

        // Calibrated LAPIC timer frequency
        let lapic_freq_str = match lapic_timer_freq_hz() {
            Some(freq) => format!("{} Hz ({:.2} MHz)", freq, freq as f64 / 1_000_000.0),
            None => "Not calibrated".into(),
        };
        lines.push(Line::raw(format!("LAPIC Timer Freq: {}", lapic_freq_str)));

        lines.push(Line::raw(format!("Target Timer Hz:  {}", TARGET_TIMER_HZ)));
        lines.push(Line::raw(format!("Current Ticks:    {}", interrupts::tick_count())));
        lines.push(Line::raw(""));

        // Color cycling test element
        lines.push(Line::styled(
            "● Color cycles every 2 seconds",
            Style::default().fg(self.color()),
        ));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }

    fn render_fpu_pane(&mut self, area: Rect, buf: &mut Buffer) {
        let fp_area = self.fxsave64();

        let header = Line::styled("fxsave64", Style::default().bold());
        let line = Line::raw(format!("mcxsr=0x{:x}", fp_area.0.mxcsr));
        let mut text = vec![header, line];
        for i in 0..16 {
            let value = XmmBytes(fp_area.0.xmm[i]);
            let line = format!("xmm{:02}={:x}", i, value);
            text.push(Line::raw(line));
        }

        // Display YMM registers if AVX2 is available
        if self.cpuid_state.has_avx2() {
            text.push(Line::raw(""));
            text.push(Line::styled("AVX2 YMM Registers", Style::default().bold()));
            let mut ymm_regs = YmmRegisters::new_zeroed();
            read_ymm_registers(&mut ymm_regs);
            for i in 0..16 {
                let value = YmmBytes(ymm_regs.ymm[i]);
                let line = format!("ymm{:02}={:x}", i, value);
                text.push(Line::raw(line));
            }
        }

        let n_lines = text.len();
        let paragraph = Paragraph::new(Text::from(text)).scroll((self.scroll_hints.fpu.y_offset, 0));
        paragraph.render(area, buf);

        self.scroll_hints.fpu.max_offset = (n_lines as u16).saturating_sub(area.height);
        self.scroll_hints.fpu.page_height = area.height.saturating_sub(2);
    }

    fn render_xsave_pane(&self, area: Rect, buf: &mut Buffer) {
        let state = &self.cpuid_state;
        let line_1 = format!("Leaf 0x1 reports XSAVE: {}", state.has_xsave());
        let [eax, ebx, ecx, edx] = state.leaf_0x1_0();
        let line_2 = format!(
            "Leaf 0x1:0 -> EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x}",
            eax, ebx, ecx, edx
        );
        let [eax, ebx, ecx, edx] = state.leaf_0xd_0();
        let line_3 = format!(
            "Leaf 0xD:0 -> EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x}",
            eax, ebx, ecx, edx
        );
        let [eax, ..] = state.leaf_0xd_1();
        let line_4 = format!(
            "Leaf 0xD:1 -> EAX={:08x} (bit 1 XSAVEC={})",
            eax,
            (eax >> 1) & 1
        );

        let lines = vec![line_1, line_2, line_3, line_4]
            .into_iter()
            .map(Line::raw)
            .collect::<Vec<Line>>();
        let paragraph = Paragraph::new(lines);

        paragraph.render(area, buf);
    }

    fn render_cpuid_pane(&mut self, area: Rect, buf: &mut Buffer) {
        let state = &self.cpuid_state;
        let vendor_info = state.vendor_info();
        let vendor_header: Line = Line::styled("CPU Vendor:", Style::default().bold());
        let amd = if vendor_info.amd { "Yes" } else { "No" };
        let amd_line = Line::raw(format!("{:<8} = {}", "AMD", amd));
        let intel = if vendor_info.intel { "Yes" } else { "No" };
        let intel_line = Line::raw(format!("{:<8} = {}", "Intel", intel));
        let mut lines = vec![vendor_header, amd_line, intel_line];

        let empty_line = Line::raw("");
        lines.push(empty_line.clone());

        let features_header = Line::styled("CPU Features:", Style::default().bold());
        lines.push(features_header);
        for feature in state.features() {
            let yes_no = if feature.1 { "Yes" } else { "No" };
            let line = Line::raw(format!("{:<16} = {}", feature.0, yes_no));
            lines.push(line);
        }

        lines.push(empty_line.clone());

        let extended_features_header = Line::styled("Extended Features:", Style::default().bold());
        lines.push(extended_features_header);
        for extended_feature in state.extended_features() {
            let yes_no = if extended_feature.1 { "Yes" } else { "No" };
            let line = Line::raw(format!("{:<16} = {}", extended_feature.0, yes_no));
            lines.push(line);
        }

        lines.push(empty_line.clone());

        let extended_state_features_header =
            Line::styled("Extended State Features:", Style::default().bold());
        lines.push(extended_state_features_header);
        let esf = state.extended_state_features();
        for feature in esf.supports() {
            let yes_no = if feature.1 { "Yes" } else { "No" };
            let line = Line::raw(format!("{:<30} = {}", feature.0, yes_no));
            lines.push(line);
        }

        lines.push(empty_line);

        for size_feature in esf.sizes() {
            let line = Line::raw(format!("{:<34} = {} bytes", size_feature.0, size_feature.1));
            lines.push(line);
        }

        let n_lines = lines.len();
        let paragraph = Paragraph::new(lines).scroll((self.scroll_hints.cpuid.y_offset, 0));

        paragraph.render(area, buf);

        self.scroll_hints.cpuid.max_offset = (n_lines as u16).saturating_sub(area.height);
        self.scroll_hints.cpuid.page_height = area.height.saturating_sub(2);
    }
}

impl Widget for &mut App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let pane_block = Block::default()
            .title(self.pane_title())
            .borders(Borders::ALL);

        let [pane_area, bottom_bar] = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Fill(1), Constraint::Length(1)])
            .areas(area);

        let block_inner = pane_block.inner(pane_area);
        pane_block.render(pane_area, buf);

        match self.pane {
            Pane::Fpu => self.render_fpu_pane(block_inner, buf),
            Pane::Xsave => self.render_xsave_pane(block_inner, buf),
            Pane::Cpuid => self.render_cpuid_pane(block_inner, buf),
            Pane::Timer => self.render_timer_pane(block_inner, buf),
        }

        let caption = "CPUID (c) | FPU (f) | XSAVE (x) | Timer (t) | Quit (q)";
        caption.render(bottom_bar, buf);
    }
}
