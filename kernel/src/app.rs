use alloc::format;
use alloc::string::String;
use alloc::vec;
use core::sync::atomic::Ordering;
use x86_64::instructions;

use crate::cpuid::CpuidPane;
use crate::fpu::FpuState;
use crate::input::{Input, InputEvent};
use crate::interrupts;
#[cfg(feature = "msr")]
use crate::msr::MsrPane;
use crate::pane::{ScrollDirection, Scrollable, Searchable, MIN_SEARCH_LEN};
use crate::qemu::{self, QemuExitCode};
use crate::ratatui_backend::SerialAnsiBackend;
use crate::serial::{self, SerialPort};
use crate::timer::TimerState;
use crate::xsave::XsaveState;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};
use ratatui::Terminal;

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

pub struct App {
    pane: Pane,
    cpuid_pane: CpuidPane,
    fpu_state: FpuState,
    xsave_state: XsaveState,
    timer_state: TimerState,
    #[cfg(feature = "msr")]
    msr_pane: MsrPane,
    mode: Mode,
    search_buffer: String,
}

impl App {
    pub fn new() -> Self {
        let cpuid_pane = CpuidPane::new();

        let timer_state = TimerState::new(
            cpuid_pane.state().leaf(0x15, 0),
            cpuid_pane.state().leaf(0x16, 0),
        );

        #[cfg(feature = "msr")]
        let msr_pane = MsrPane::new(cpuid_pane.state().cpu_features());

        let fpu_state = FpuState::new(cpuid_pane.state());
        let xsave_state = XsaveState::new(cpuid_pane.state());

        Self {
            pane: Pane::Cpuid,
            cpuid_pane,
            fpu_state,
            xsave_state,
            timer_state,
            #[cfg(feature = "msr")]
            msr_pane,
            mode: Mode::default(),
            search_buffer: String::new(),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn timer_state(&self) -> &TimerState {
        &self.timer_state
    }

    pub fn pane(&self) -> Pane {
        self.pane
    }

    fn scroll(&mut self, direction: ScrollDirection) {
        match self.pane {
            Pane::Cpuid => self.cpuid_pane.scroll(direction),
            Pane::Fpu => self.fpu_state.scroll(direction),
            #[cfg(feature = "msr")]
            Pane::Msr => self.msr_pane.scroll(direction),
            _ => {}
        }
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
        let second_events = interrupts::SECOND_EVENTS.swap(0, Ordering::AcqRel);
        second_events > 0
    }

    /// Perform search on current pane
    fn perform_search(&mut self) {
        let query = &self.search_buffer;

        // Check minimum length
        if query.len() < MIN_SEARCH_LEN {
            return;
        }

        match self.pane {
            Pane::Cpuid => self.cpuid_pane.perform_search(query),
            #[cfg(feature = "msr")]
            Pane::Msr => self.msr_pane.perform_search(query),
            _ => {}
        }
    }

    fn next_match(&mut self) {
        match self.pane {
            Pane::Cpuid => self.cpuid_pane.next_match(),
            #[cfg(feature = "msr")]
            Pane::Msr => self.msr_pane.next_match(),
            _ => {}
        }
    }

    fn prev_match(&mut self) {
        match self.pane {
            Pane::Cpuid => self.cpuid_pane.prev_match(),
            #[cfg(feature = "msr")]
            Pane::Msr => self.msr_pane.prev_match(),
            _ => {}
        }
    }

    fn clear_search(&mut self) {
        match self.pane {
            Pane::Cpuid => self.cpuid_pane.clear_search(),
            #[cfg(feature = "msr")]
            Pane::Msr => self.msr_pane.clear_search(),
            _ => {}
        }
    }

    /// Get current pane's search match info (current, total), None if not searchable
    fn search_match_info(&self) -> Option<(usize, usize)> {
        match self.pane {
            Pane::Cpuid => {
                let s = self.cpuid_pane.search_state();
                Some((s.current_match + 1, s.matches.len()))
            }
            #[cfg(feature = "msr")]
            Pane::Msr => {
                let s = self.msr_pane.search_state();
                Some((s.current_match + 1, s.matches.len()))
            }
            _ => None,
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

            /* update at least once per second */
            self.timer_state.refresh();
            let mut needs_redraw = self.handle_ticks();

            /* react to input */
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
                        self.clear_search();
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
                    InputEvent::ClearScreen => {
                        terminal.clear().unwrap();
                    }
                }
                needs_redraw = true;
            }

            if needs_redraw {
                self.draw(&mut terminal);
                needs_redraw = false;
            }
        }
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
            Pane::Xsave => (&self.xsave_state).render(block_inner, buf),
            Pane::Cpuid => (&mut self.cpuid_pane).render(block_inner, buf),
            Pane::Timer => (&self.timer_state).render(block_inner, buf),
            #[cfg(feature = "msr")]
            Pane::Msr => (&mut self.msr_pane).render(block_inner, buf),
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
            let (current, match_count) = self.search_match_info().unwrap_or((0, 0));
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
