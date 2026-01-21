use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::cpuid;
use crate::interrupts::tick_count;
use crate::lapic::{TARGET_TIMER_HZ, lapic_timer_freq_hz, read_lapic_timer_regs};

pub struct TimerState {
    tick_count: usize,
    leaf_0x15: [u32; 4],
    leaf_0x16: [u32; 4],
}

impl TimerState {
    pub fn new(leaf_0x15: [u32; 4], leaf_0x16: [u32; 4]) -> Self {
        Self {
            tick_count: 0,
            leaf_0x15,
            leaf_0x16,
        }
    }

    fn color(&self) -> Color {
        const FACTOR: usize = (TARGET_TIMER_HZ / 2) as usize;
        let hue = ((self.tick_count / FACTOR) as u16) % 360;
        let (r, g, b) = hsv_to_rgb(hue);
        Color::Rgb(r, g, b)
    }

    pub fn refresh(&mut self) {
        self.tick_count = tick_count();
    }
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

impl Widget for &TimerState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = vec![Line::styled("Timer Calibration", Style::default().bold())];

        // TSC frequency from CPUID
        let tsc_freq_str: String = match cpuid::tsc_frequency() {
            Some(freq) => format!("{} Hz ({:.2} GHz)", freq, freq as f64 / 1_000_000_000.0),
            None => "Not available".into(),
        };
        lines.push(Line::raw(format!(
            "{:<18}{}",
            "TSC Frequency:", tsc_freq_str
        )));

        // Calibrated LAPIC timer frequency
        let lapic_freq_str: String = match lapic_timer_freq_hz() {
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

        // LAPIC Timer Registers section
        lines.push(Line::styled(
            "LAPIC Timer Registers",
            Style::default().bold(),
        ));
        lines.push(Line::raw(format!(
            "{:<18}{:<14}{}",
            "Register", "MSR", "Value"
        )));
        let r = read_lapic_timer_regs();
        lines.push(Line::raw(format!(
            "{:<18}{:<14}0x{:08X}",
            "LVT Timer", "0x832", r.lvt_timer
        )));
        lines.push(Line::raw(format!(
            "{:<18}{:<14}0x{:08X} ({})",
            "Initial Count", "0x838", r.initial_count, r.initial_count
        )));
        lines.push(Line::raw(format!(
            "{:<18}{:<14}0x{:08X} ({})",
            "Current Count", "0x839", r.current_count, r.current_count
        )));
        lines.push(Line::raw(format!(
            "{:<18}{:<14}0x{:08X}",
            "Divide Config", "0x83E", r.divide_config
        )));
        lines.push(Line::raw(""));

        // Raw CPUID leaf diagnostics
        lines.push(Line::styled("CPUID Diagnostics", Style::default().bold()));
        let [eax, ebx, ecx, _] = self.leaf_0x15;
        lines.push(Line::raw(format!(
            "Leaf 0x15: denom={} numer={} crystal_hz={}",
            eax, ebx, ecx
        )));
        let [eax, ebx, ecx, _] = self.leaf_0x16;
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
            format!("{} hours", total_seconds / 3600)
        } else if total_seconds >= 120 {
            format!("{} minutes", total_seconds / 60)
        } else if total_seconds >= 60 {
            format!("1 minute {} seconds", total_seconds - 60)
        } else {
            format!("{} seconds", total_seconds)
        };
        let uptime_span = Span::styled(uptime_str, Style::default().fg(self.color()));
        lines.push(Line::from(vec![Span::raw("Uptime: "), uptime_span]));

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}
