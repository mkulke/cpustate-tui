use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use x86_64::instructions;

use crate::cpuid::{self, CpuFeatures, ExtendedStateFeatures, VendorInfo};
use crate::fpu::{enable_avx, enable_sse, set_xmm0_bytes, set_xmm15_bytes, FpuState};
use crate::input::{Input, InputEvent};
use crate::interrupts;
use crate::lapic::{lapic_timer_freq_hz, TARGET_TIMER_HZ};
#[cfg(feature = "msr")]
use crate::msr::{self, MsrCategory};
use crate::qemu::{self, QemuExitCode};
use crate::ratatui_backend::SerialAnsiBackend;
use crate::scroll::{ScrollDirection, ScrollHints};
use crate::serial::{self, SerialPort};

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use ratatui::Terminal;

pub struct CpuidState {
    features: CpuFeatures,
}

impl CpuidState {
    fn new() -> Self {
        let features = CpuFeatures::new();

        Self { features }
    }

    fn features(&self) -> &Vec<(&'static str, bool)> {
        self.features.features()
    }

    fn extended_features(&self) -> &Vec<(&'static str, bool)> {
        self.features.extended_features()
    }

    fn extended_state_features(&self) -> &ExtendedStateFeatures {
        self.features.extended_state_features()
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

    fn leaf(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        self.features.leaf(leaf, subleaf)
    }

    fn cpu_features(&self) -> &CpuFeatures {
        &self.features
    }
}

#[derive(PartialEq, Clone, Copy)]
pub enum Pane {
    Cpuid,
    Fpu,
    Xsave,
    Timer,
    #[cfg(feature = "msr")]
    Msr,
}

#[derive(Default, PartialEq, Clone, Copy)]
pub enum Mode {
    #[default]
    Navigation,
    Search,
    SearchResults,
}

#[derive(Default)]
struct PaneScrollHints {
    cpuid: ScrollHints,
    #[cfg(feature = "msr")]
    msr: ScrollHints,
}

/// Minimum characters before search activates
const MIN_SEARCH_LEN: usize = 2;

#[derive(Default)]
struct SearchState {
    inner: search::SearchState,
}

pub struct App {
    hue: u16,
    pane: Pane,
    cpuid_state: CpuidState,
    fpu_state: FpuState,
    #[cfg(feature = "msr")]
    msr_state: Vec<MsrCategory>,
    scroll_hints: PaneScrollHints,
    mode: Mode,
    search_buffer: String,
    search_state: SearchState,
    tick_count: usize,
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

impl App {
    pub fn new() -> Self {
        enable_sse();
        write_xmm_values();

        let cpuid_state = CpuidState::new();
        let has_avx2 = cpuid_state.has_avx2();

        // Enable AVX if the CPU supports AVX2
        if has_avx2 {
            enable_avx();
        }

        #[cfg(feature = "msr")]
        let msr_state = msr::read_all_msrs(cpuid_state.cpu_features());

        Self {
            hue: 0,
            pane: Pane::Cpuid,
            cpuid_state,
            fpu_state: FpuState::new(has_avx2),
            #[cfg(feature = "msr")]
            msr_state,
            scroll_hints: PaneScrollHints::default(),
            mode: Mode::default(),
            search_buffer: String::new(),
            search_state: SearchState::default(),
            tick_count: 0,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn pane(&self) -> Pane {
        self.pane
    }

    pub fn tick_count(&self) -> usize {
        self.tick_count
    }

    fn tick(&mut self) {
        // Advance hue by 5 degrees every 500ms (full cycle in 72 ticks = 36 seconds)
        self.hue = (self.hue + 5) % 360;
    }

    /// Convert HSV to RGB. Hue: 0-359, Saturation/Value: fixed at 1.0 for vibrant colors.
    fn hsv_to_rgb(hue: u16) -> (u8, u8, u8) {
        let h = hue % 360;
        let sector = h / 60;
        let f = (h % 60) as u32 * 255 / 60; // fractional part scaled to 0-255

        let (r, g, b) = match sector {
            0 => (255, f as u8, 0),       // Red -> Yellow
            1 => (255 - f as u8, 255, 0), // Yellow -> Green
            2 => (0, 255, f as u8),       // Green -> Cyan
            3 => (0, 255 - f as u8, 255), // Cyan -> Blue
            4 => (f as u8, 0, 255),       // Blue -> Magenta
            _ => (255, 0, 255 - f as u8), // Magenta -> Red
        };
        (r, g, b)
    }

    fn color(&self) -> Color {
        let (r, g, b) = Self::hsv_to_rgb(self.hue);
        Color::Rgb(r, g, b)
    }

    fn scroll(&mut self, direction: ScrollDirection) {
        let hints = match self.pane {
            Pane::Cpuid => &mut self.scroll_hints.cpuid,
            Pane::Fpu => &mut self.fpu_state.scroll,
            #[cfg(feature = "msr")]
            Pane::Msr => &mut self.scroll_hints.msr,
            _ => return,
        };
        hints.scroll(direction);
    }

    fn pane_title(&self) -> &'static str {
        match self.pane {
            Pane::Cpuid => "CPUID",
            Pane::Fpu => "FPU",
            Pane::Xsave => "XSAVE",
            Pane::Timer => "Timer",
            #[cfg(feature = "msr")]
            Pane::Msr => "MSR",
        }
    }

    fn handle_input(&mut self, input: &mut Input) -> Option<InputEvent> {
        let mut event = None;

        serial::RX_QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            let (_prod, mut cons) = queue.split();

            let Some(byte) = cons.dequeue() else {
                return;
            };

            event = input.handle_byte(self, byte);
        });

        event
    }

    fn handle_ticks(&mut self) -> bool {
        let color_events = interrupts::PRINT_EVENTS.swap(0, Ordering::AcqRel);
        for _ in 0..color_events {
            self.tick();
        }

        let second_events = interrupts::SECOND_EVENTS.swap(0, Ordering::AcqRel);
        color_events > 0 || second_events > 0
    }

    /// Perform search on CPUID features, updating matches and scrolling to first match
    fn perform_search(&mut self) {
        let query = self.search_buffer.clone();

        // Check minimum length
        if query.len() < MIN_SEARCH_LEN {
            self.search_state.inner.matches.clear();
            return;
        }

        // Skip if query hasn't changed
        if query == self.search_state.inner.last_query {
            return;
        }

        self.search_state.inner.last_query = query.clone();
        self.search_state.inner.matches.clear();
        self.search_state.inner.current_match = 0;

        match self.pane {
            Pane::Cpuid => self.perform_cpuid_search(&query),
            #[cfg(feature = "msr")]
            Pane::Msr => self.perform_msr_search(&query),
            _ => {}
        }
    }

    fn perform_cpuid_search(&mut self, query: &str) {
        // Build search index: line numbers where features match
        // Line structure in CPUID pane:
        // 0: vendor header, 1: amd, 2: intel, 3: empty
        // 4: features header, 5+: features...
        let mut line: u16 = 5; // Start after vendor section + features header

        // Features
        self.search_state.inner.matches.extend(search::find_matches(
            query,
            self.cpuid_state.features(),
            line,
        ));
        line += self.cpuid_state.features().len() as u16;

        // Empty line + extended features header
        line += 2;
        self.search_state.inner.matches.extend(search::find_matches(
            query,
            self.cpuid_state.extended_features(),
            line,
        ));
        line += self.cpuid_state.extended_features().len() as u16;

        // Empty line + extended state features header
        line += 2;
        self.search_state.inner.matches.extend(search::find_matches(
            query,
            self.cpuid_state.extended_state_features().supports(),
            line,
        ));

        // Jump to first match if any
        if let Some(&first) = self.search_state.inner.matches.first() {
            self.scroll_hints.cpuid.y_offset = first;
        }
    }

    #[cfg(feature = "msr")]
    fn perform_msr_search(&mut self, query: &str) {
        // Build search index for MSR pane
        // Each category has: header line, N entry lines, empty line
        let mut line: u16 = 0;

        for category in &self.msr_state {
            // Skip header line
            line += 1;

            // Collect entry names for this category
            let names: Vec<&str> = category.entries.iter().map(|e| e.name).collect();
            self.search_state
                .inner
                .matches
                .extend(search::find_matches_strs(query, &names, line));

            line += category.entries.len() as u16;
            // Empty line
            line += 1;
        }

        // Jump to first match if any
        if let Some(&first) = self.search_state.inner.matches.first() {
            self.scroll_hints.msr.y_offset = first;
        }
    }

    fn next_match(&mut self) {
        let Some(offset) = self.search_state.inner.next_match() else {
            return;
        };
        match self.pane {
            Pane::Cpuid => self.scroll_hints.cpuid.scroll_to(offset),
            #[cfg(feature = "msr")]
            Pane::Msr => self.scroll_hints.msr.scroll_to(offset),
            _ => {}
        }
    }

    fn prev_match(&mut self) {
        let Some(offset) = self.search_state.inner.prev_match() else {
            return;
        };
        match self.pane {
            Pane::Cpuid => self.scroll_hints.cpuid.scroll_to(offset),
            #[cfg(feature = "msr")]
            Pane::Msr => self.scroll_hints.msr.scroll_to(offset),
            _ => {}
        }
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

        let mut input = Input::new();

        loop {
            instructions::hlt();

            self.tick_count = interrupts::tick_count();

            let mut needs_redraw = self.handle_ticks();

            let event = self.handle_input(&mut input);
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
                    InputEvent::EnterSearchMode => {
                        self.mode = Mode::Search;
                        self.search_buffer.clear();
                        self.search_state.inner.clear();
                    }
                    InputEvent::ConfirmSearch => self.mode = Mode::SearchResults,
                    InputEvent::ExitSearchMode => self.mode = Mode::Navigation,
                    InputEvent::SearchInput(b) => {
                        // Limit search buffer length (leave room for "/" prefix)
                        if self.search_buffer.len() < 76 {
                            self.search_buffer.push(b as char);
                            self.perform_search();
                        }
                    }
                    InputEvent::SearchBackspace => {
                        self.search_buffer.pop();
                        self.perform_search();
                    }
                    InputEvent::NextMatch => self.next_match(),
                    InputEvent::PrevMatch => self.prev_match(),
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
        lines.push(Line::raw(format!(
            "{:<18}{}",
            "TSC Frequency:", tsc_freq_str
        )));

        // Calibrated LAPIC timer frequency
        let lapic_freq_str = match lapic_timer_freq_hz() {
            Some(freq) => format!("{} Hz ({:.2} MHz)", freq, freq as f64 / 1_000_000.0),
            None => "Not calibrated".into(),
        };
        lines.push(Line::raw(format!(
            "{:<18}{}",
            "LAPIC Timer Freq:", lapic_freq_str
        )));

        lines.push(Line::raw(format!(
            "{:<18}{}",
            "Target Timer Hz:", TARGET_TIMER_HZ
        )));
        lines.push(Line::raw(format!(
            "{:<18}{}",
            "Current Ticks:", self.tick_count
        )));
        lines.push(Line::raw(""));

        // Raw CPUID leaf diagnostics
        lines.push(Line::styled("CPUID Diagnostics", Style::default().bold()));
        let [eax, ebx, ecx, _] = self.cpuid_state.leaf(0x15, 0);
        lines.push(Line::raw(format!(
            "Leaf 0x15: denom={} numer={} crystal_hz={}",
            eax, ebx, ecx
        )));
        let [eax, ebx, ecx, _] = self.cpuid_state.leaf(0x16, 0);
        lines.push(Line::raw(format!(
            "Leaf 0x16: base={}MHz max={}MHz bus={}MHz",
            eax & 0xFFFF,
            ebx & 0xFFFF,
            ecx & 0xFFFF
        )));
        lines.push(Line::raw(""));

        // Misc section with uptime
        lines.push(Line::styled("Misc", Style::default().bold()));
        let total_seconds = self.tick_count as u64 / TARGET_TIMER_HZ;
        let uptime_str = if total_seconds >= 120 * 60 {
            // >= 120 minutes: show hours only
            format!("{} hours", total_seconds / 3600)
        } else if total_seconds >= 120 {
            // >= 120 seconds: show minutes only
            format!("{} minutes", total_seconds / 60)
        } else if total_seconds >= 60 {
            // 60-119 seconds: show "1 minute X seconds"
            format!("1 minute {} seconds", total_seconds - 60)
        } else {
            format!("{} seconds", total_seconds)
        };
        let uptime_span = Span::styled(uptime_str, Style::default().fg(self.color()));
        lines.push(Line::from(vec![Span::raw("Uptime: "), uptime_span]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
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

    #[cfg(feature = "msr")]
    fn render_msr_pane(&mut self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();
        let num_categories = self.msr_state.len();

        for (i, category) in self.msr_state.iter().enumerate() {
            // Category header
            lines.push(Line::styled(category.name, Style::default().bold()));

            for entry in &category.entries {
                let value_str = match entry.value {
                    Some(v) => format!("0x{:016x}", v),
                    None => "N/A".to_string(),
                };
                lines.push(self.highlight_msr_line(entry.name, entry.address, &value_str));
            }

            // Empty line between categories (but not after the last one)
            if i < num_categories - 1 {
                lines.push(Line::raw(""));
            }
        }

        let n_lines = lines.len();
        let paragraph = Paragraph::new(lines).scroll((self.scroll_hints.msr.y_offset, 0));

        paragraph.render(area, buf);

        self.scroll_hints.msr.update_from_render(n_lines, area.height);
    }

    /// Create a Line for MSR entry with search term highlighted
    #[cfg(feature = "msr")]
    fn highlight_msr_line(&self, name: &str, address: u32, value: &str) -> Line<'_> {
        let suffix = format!(" (0x{:08X}) = {}", address, value);

        // Only highlight in Search or SearchResults mode
        if self.mode != Mode::Search && self.mode != Mode::SearchResults {
            return Line::raw(format!("{:<24}{}", name, suffix));
        }

        let query = &self.search_buffer;

        if query.len() >= MIN_SEARCH_LEN && search::smart_contains(name, query) {
            let pos = if search::has_uppercase(query) {
                name.find(query.as_str())
            } else {
                name.to_lowercase().find(&query.to_lowercase())
            };

            if let Some(pos) = pos {
                let before = &name[..pos];
                let matched = &name[pos..pos + query.len()];
                let after = &name[pos + query.len()..];

                // Pad name to 24 chars
                let padding = 24usize.saturating_sub(name.len());
                let padded_after = format!("{}{}", after, " ".repeat(padding));

                return Line::from(vec![
                    Span::raw(before.to_string()),
                    Span::styled(
                        matched.to_string(),
                        Style::default().fg(Color::Black).bg(Color::Yellow),
                    ),
                    Span::raw(padded_after),
                    Span::raw(suffix),
                ]);
            }
        }

        Line::raw(format!("{:<24}{}", name, suffix))
    }

    /// Create a Line with search term highlighted (only in Search/SearchResults mode)
    fn highlight_line(&self, name: &str, value: &str, width: usize) -> Line<'_> {
        // Only highlight in Search or SearchResults mode
        if self.mode != Mode::Search && self.mode != Mode::SearchResults {
            return Line::raw(format!("{:<width$} = {}", name, value, width = width));
        }

        let query = &self.search_buffer;

        if query.len() >= MIN_SEARCH_LEN && search::smart_contains(name, query) {
            // Find match position (need to handle smart-case)
            let pos = if search::has_uppercase(query) {
                name.find(query.as_str())
            } else {
                name.to_lowercase().find(&query.to_lowercase())
            };

            if let Some(pos) = pos {
                let before = &name[..pos];
                let matched = &name[pos..pos + query.len()];
                let after = &name[pos + query.len()..];

                // Pad to width
                let padding = width.saturating_sub(name.len());
                let padded_after = format!("{}{}", after, " ".repeat(padding));

                return Line::from(vec![
                    Span::raw(before.to_string()),
                    Span::styled(
                        matched.to_string(),
                        Style::default().fg(Color::Black).bg(Color::Yellow),
                    ),
                    Span::raw(padded_after),
                    Span::raw(format!(" = {}", value)),
                ]);
            }
        }

        Line::raw(format!("{:<width$} = {}", name, value, width = width))
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
        for feature in state.features().clone() {
            let yes_no = if feature.1 { "Yes" } else { "No" };
            lines.push(self.highlight_line(feature.0, yes_no, 16));
        }

        lines.push(empty_line.clone());

        let extended_features_header = Line::styled("Extended Features:", Style::default().bold());
        lines.push(extended_features_header);
        for extended_feature in state.extended_features().clone() {
            let yes_no = if extended_feature.1 { "Yes" } else { "No" };
            lines.push(self.highlight_line(extended_feature.0, yes_no, 16));
        }

        lines.push(empty_line.clone());

        let extended_state_features_header =
            Line::styled("Extended State Features:", Style::default().bold());
        lines.push(extended_state_features_header);
        let esf = state.extended_state_features();
        for feature in esf.supports().clone() {
            let yes_no = if feature.1 { "Yes" } else { "No" };
            lines.push(self.highlight_line(feature.0, yes_no, 30));
        }

        lines.push(empty_line);

        for size_feature in esf.sizes() {
            let line = Line::raw(format!("{:<34} = {} bytes", size_feature.0, size_feature.1));
            lines.push(line);
        }

        let n_lines = lines.len();
        let paragraph = Paragraph::new(lines).scroll((self.scroll_hints.cpuid.y_offset, 0));

        paragraph.render(area, buf);

        self.scroll_hints.cpuid.update_from_render(n_lines, area.height);
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
            Pane::Fpu => (&mut self.fpu_state).render(block_inner, buf),
            Pane::Xsave => self.render_xsave_pane(block_inner, buf),
            Pane::Cpuid => self.render_cpuid_pane(block_inner, buf),
            Pane::Timer => self.render_timer_pane(block_inner, buf),
            #[cfg(feature = "msr")]
            Pane::Msr => self.render_msr_pane(block_inner, buf),
        }

        if self.mode == Mode::Search {
            let search_line = Line::from(vec![
                Span::styled("/", Style::default().bold()),
                Span::raw(self.search_buffer.as_str()),
                Span::styled("_", Style::default().fg(Color::Gray)), // cursor
            ]);
            search_line.render(bottom_bar, buf);
        } else if self.mode == Mode::SearchResults {
            // Show search bar without cursor, indicate n/N navigation
            let match_count = self.search_state.inner.matches.len();
            let current = self.search_state.inner.current_match + 1;
            let search_line = Line::from(vec![
                Span::styled("/", Style::default().bold()),
                Span::raw(self.search_buffer.as_str()),
                Span::styled(
                    format!(" [{}/{}]", current, match_count),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            search_line.render(bottom_bar, buf);
        } else {
            #[cfg(feature = "msr")]
            let caption = "CPUID (c) | FPU (f) | XSAVE (x) | Timer (t) | MSR (m) | Quit (q)";
            #[cfg(not(feature = "msr"))]
            let caption = "CPUID (c) | FPU (f) | XSAVE (x) | Timer (t) | Quit (q)";
            caption.render(bottom_bar, buf);
        }
    }
}
