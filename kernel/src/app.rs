use alloc::vec::Vec;
use ratatui::style::Color;

use crate::fpu::{enable_sse, fxsave64, set_xmm0_bytes, set_xmm15_bytes, FxSaveAligned};
use crate::cpuid::{CpuFeatures, VendorInfo};

pub struct CpuidState {
    y_offset: u16,
    features: CpuFeatures,
}

impl CpuidState {
    fn new() -> Self {
        let features = CpuFeatures::new();
        let y_offset = 0;

        Self {
            y_offset,
            features,
        }
    }

    pub fn features(&self) -> &Vec<(&'static str, bool)> {
        &self.features.features()
    }

    pub fn extended_features(&self) -> &Vec<(&'static str, bool)> {
        &self.features.extended_features()
    }

    pub fn vendor_info(&self) -> &VendorInfo {
        self.features.vendor_info()
    }

    pub fn y_offset(&self) -> u16 {
        self.y_offset
    }
}

pub enum Pane {
    Cpuid,
    Fpu,
    Xsave,
}

pub struct App {
    color_idx: usize,
    pane: Pane,
    cpuid_state: CpuidState,
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

        Self {
            color_idx: 0,
            pane: Pane::Cpuid,
            cpuid_state,
        }
    }

    pub fn pane(&self) -> &Pane {
        &self.pane
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

    pub fn set_pane(&mut self, pane: Pane) {
        self.pane = pane;
    }

    pub fn scroll_up(&mut self) {
        if let Pane::Cpuid = self.pane {
            let offset = &mut self.cpuid_state.y_offset;
            *offset = offset.saturating_sub(1);
        }
    }

    pub fn scroll_down(&mut self, max_offset: Option<u16>) {
        if let Pane::Cpuid = self.pane {
            let offset = &mut self.cpuid_state.y_offset;
            if let Some(max) = max_offset && *offset == max {
                return;
            }
            *offset = offset.saturating_add(1);
        }
    }

    pub fn pane_title(&self) -> &'static str {
        match self.pane {
            Pane::Cpuid => "CPUID",
            Pane::Fpu => "FPU",
            Pane::Xsave => "XSAVE",
        }
    }

    pub fn fxsave64(&self) -> FxSaveAligned {
        let mut area = FxSaveAligned::new_zeroed();
        fxsave64(&mut area);
        area
    }

    pub fn cpuid_state(&self) -> &CpuidState {
        &self.cpuid_state
    }
}
