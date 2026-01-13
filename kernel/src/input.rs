use crate::app::{App, Mode, Pane};
use crate::interrupts;
use crate::lapic::TARGET_TIMER_HZ;

const SEQUENCE_TIMEOUT_TICKS: usize = (TARGET_TIMER_HZ / 2) as usize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sequence {
    #[allow(non_camel_case_types)]
    gg,
}

pub enum InputEvent {
    Quit,
    ScrollToTop,
    ScrollToBottom,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    SelectPane(Pane),
    EnterSearchMode,
    ConfirmSearch,
    ExitSearchMode,
    SearchInput(u8),
    SearchBackspace,
    NextMatch,
    PrevMatch,
}

pub struct Input {
    in_sequence: Option<Sequence>,
    in_sequence_since: usize,
}

impl Input {
    pub fn new() -> Self {
        Self {
            in_sequence: None,
            in_sequence_since: 0,
        }
    }

    fn handle_sequence(&mut self, seq: Sequence) -> bool {
        let now = interrupts::tick_count();
        // no sequence in progress
        let Some(seq) = &mut self.in_sequence else {
            self.in_sequence = Some(seq);
            self.in_sequence_since = now;
            return false;
        };
        // abort other sequence
        if *seq != Sequence::gg {
            self.in_sequence = None;
            return false;
        };
        // timeout expired, restart sequence
        let delta = now.saturating_sub(self.in_sequence_since);
        if delta > SEQUENCE_TIMEOUT_TICKS {
            self.in_sequence_since = now;
            return false;
        }
        // complete sequence
        self.in_sequence = None;
        true
    }

    pub fn handle_byte(&mut self, app: &App, byte: u8) -> Option<InputEvent> {
        match app.mode() {
            // Search input mode - typing in search bar
            Mode::Search => {
                match byte {
                    0x1B => Some(InputEvent::ExitSearchMode),         // ESC
                    0x7F | 0x08 => Some(InputEvent::SearchBackspace), // Backspace/DEL
                    // Enter - confirm and go to results mode
                    0x0D => Some(InputEvent::ConfirmSearch),
                    // Printable ASCII
                    b if (0x20..0x7F).contains(&b) => Some(InputEvent::SearchInput(b)),
                    _ => None,
                }
            }
            Mode::SearchResults => {
                // Search results mode - n/N navigation, ESC/Enter to exit
                match byte {
                    0x1B | 0x0D => Some(InputEvent::ExitSearchMode), // ESC or Enter exits to Navigation
                    b'n' => Some(InputEvent::NextMatch),
                    b'N' => Some(InputEvent::PrevMatch),
                    b'/' => Some(InputEvent::EnterSearchMode), // Start new search
                    _ => None,
                }
            }
            Mode::Navigation => {
                // Navigation mode input handling
                match byte {
                    b'q' => Some(InputEvent::Quit),
                    b'/' if app.pane() == Pane::Cpuid => Some(InputEvent::EnterSearchMode),
                    #[cfg(feature = "msr")]
                    b'/' if app.pane() == Pane::Msr => Some(InputEvent::EnterSearchMode),
                    b'c' => Some(InputEvent::SelectPane(Pane::Cpuid)),
                    b'f' => Some(InputEvent::SelectPane(Pane::Fpu)),
                    b'x' => Some(InputEvent::SelectPane(Pane::Xsave)),
                    b't' => Some(InputEvent::SelectPane(Pane::Timer)),
                    #[cfg(feature = "msr")]
                    b'm' => Some(InputEvent::SelectPane(Pane::Msr)),
                    b'j' => Some(InputEvent::ScrollDown),
                    b'k' => Some(InputEvent::ScrollUp),
                    b'G' => Some(InputEvent::ScrollToBottom),
                    0x06 => Some(InputEvent::PageDown), // Ctrl+F
                    0x02 => Some(InputEvent::PageUp),   // Ctrl+B
                    b'g' => {
                        let sequence_finalized = self.handle_sequence(Sequence::gg);
                        sequence_finalized.then_some(InputEvent::ScrollToTop)
                    }
                    _ => {
                        // Any other key aborts ongoing sequence
                        self.in_sequence = None;
                        None
                    }
                }
            }
        }
    }
}
