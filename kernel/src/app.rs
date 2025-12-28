use alloc::string::String;
use alloc::vec::Vec;
use ratatui::style::Color;

use crate::fpu::{enable_sse, fxsave64, set_xmm0_bytes, set_xmm15_bytes, FxSaveAligned};
use raw_cpuid::CpuId;
use raw_cpuid::CpuIdReaderNative;

fn build_features(cpuid: &CpuId<CpuIdReaderNative>) -> Vec<(&'static str, bool)> {
    let fi = cpuid.get_feature_info().unwrap();

    let mut out: Vec<(&str, bool)> = Vec::new();

    macro_rules! push_has {
        ($m:ident) => {
            let name = stringify!($m).strip_prefix("has_").unwrap();
            out.push((name, fi.$m()));
        };
    }

    push_has!(has_acpi);
    push_has!(has_aesni);
    push_has!(has_apic);
    push_has!(has_avx);
    push_has!(has_clflush);
    push_has!(has_cmov);
    push_has!(has_cmpxchg8b);
    push_has!(has_cmpxchg16b);
    push_has!(has_cnxtid);
    push_has!(has_cpl);
    push_has!(has_dca);
    push_has!(has_de);
    push_has!(has_ds);
    push_has!(has_ds_area);
    push_has!(has_eist);
    push_has!(has_f16c);
    push_has!(has_fma);
    push_has!(has_fpu);
    push_has!(has_fxsave_fxstor);
    push_has!(has_htt);
    push_has!(has_hypervisor);
    push_has!(has_mca);
    push_has!(has_mce);
    push_has!(has_mmx);
    push_has!(has_monitor_mwait);
    push_has!(has_movbe);
    push_has!(has_msr);
    push_has!(has_mtrr);
    push_has!(has_oxsave);
    push_has!(has_pae);
    push_has!(has_pat);
    push_has!(has_pbe);
    push_has!(has_pcid);
    push_has!(has_pclmulqdq);
    push_has!(has_pdcm);
    push_has!(has_pge);
    push_has!(has_popcnt);
    push_has!(has_pse);
    push_has!(has_pse36);
    push_has!(has_psn);
    push_has!(has_rdrand);
    push_has!(has_smx);
    push_has!(has_ss);
    push_has!(has_sse);
    push_has!(has_sse2);
    push_has!(has_sse3);
    push_has!(has_sse41);
    push_has!(has_sse42);
    push_has!(has_ssse3);
    push_has!(has_sysenter_sysexit);
    push_has!(has_tm);
    push_has!(has_tm2);
    push_has!(has_tsc);
    push_has!(has_tsc_deadline);
    push_has!(has_vme);
    push_has!(has_vmx);
    push_has!(has_x2apic);
    push_has!(has_xsave);

    out
}
pub struct CpuidState {
    cpuid: CpuId<CpuIdReaderNative>,
    y_offset: u16,
    features: Vec<(&'static str, bool)>,
}

impl CpuidState {
    fn new() -> Self {
        let cpuid = CpuId::new();
        let features = build_features(&cpuid);
        let y_offset = 0;

        Self {
            cpuid,
            y_offset,
            features,
        }
    }

    pub fn features(&self) -> &Vec<(&'static str, bool)> {
        &self.features
    }

    pub fn vendor_info(&self) -> String {
        let info = self.cpuid.get_vendor_info().unwrap();
        info.as_str().into()
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
