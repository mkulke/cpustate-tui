//! Model Specific Register (MSR) reading and display

use alloc::format;
use alloc::vec::Vec;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::cpuid::CpuFeatures;
use crate::pane::{highlight_line, ScrollHints, Scrollable, Searchable};

/// MSR entry with name, address, and value
pub struct MsrEntry {
    pub name: &'static str,
    pub address: u32,
    pub value: Option<u64>,
}

/// Category of MSRs
pub struct MsrCategory {
    pub name: &'static str,
    pub entries: Vec<MsrEntry>,
}

/// Read an MSR (caller must ensure MSR exists)
fn read_msr(address: u32) -> u64 {
    unsafe { x86::msr::rdmsr(address) }
}

/// Read a list of MSRs by address and name
fn read_msrs(msrs: &[(&'static str, u32)]) -> Vec<MsrEntry> {
    msrs.iter()
        .map(|(name, addr)| MsrEntry {
            name,
            address: *addr,
            value: Some(read_msr(*addr)),
        })
        .collect()
}

/// Conditionally read an MSR if feature is supported
fn read_msr_if(name: &'static str, address: u32, supported: bool) -> Option<MsrEntry> {
    if supported {
        Some(MsrEntry {
            name,
            address,
            value: Some(read_msr(address)),
        })
    } else {
        None
    }
}

// MSR addresses from QEMU cpu.h
// Long mode MSRs - architectural, always present in 64-bit mode
const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_CSTAR: u32 = 0xC000_0083;
const MSR_FMASK: u32 = 0xC000_0084;
const MSR_FSBASE: u32 = 0xC000_0100;
const MSR_GSBASE: u32 = 0xC000_0101;
const MSR_KERNELGSBASE: u32 = 0xC000_0102;
const MSR_TSC_AUX: u32 = 0xC000_0103;

// System MSRs
const MSR_IA32_APICBASE: u32 = 0x1B;

// TSC MSRs
const MSR_IA32_TSC: u32 = 0x10;
const MSR_TSC_ADJUST: u32 = 0x3B;

// SYSENTER MSRs - architectural
const MSR_IA32_SYSENTER_CS: u32 = 0x174;
const MSR_IA32_SYSENTER_ESP: u32 = 0x175;
const MSR_IA32_SYSENTER_EIP: u32 = 0x176;

// Machine Check MSRs
const MSR_MCG_CAP: u32 = 0x179;
const MSR_MCG_STATUS: u32 = 0x17A;

// MTRR MSRs
const MSR_MTRRCAP: u32 = 0xFE;
const MSR_MTRR_DEF_TYPE: u32 = 0x2FF;
const MSR_MTRR_PHYSBASE0: u32 = 0x200;
const MSR_MTRR_PHYSMASK0: u32 = 0x201;
const MSR_MTRR_PHYSBASE1: u32 = 0x202;
const MSR_MTRR_PHYSMASK1: u32 = 0x203;
const MSR_MTRR_FIX64K_00000: u32 = 0x250;
const MSR_MTRR_FIX16K_80000: u32 = 0x258;
const MSR_MTRR_FIX16K_A0000: u32 = 0x259;
const MSR_MTRR_FIX4K_C0000: u32 = 0x268;

// PAT MSR
const MSR_PAT: u32 = 0x277;

/// Build all MSR categories with current values
/// Only reads MSRs that are confirmed present via CPUID
pub fn read_all_msrs(cpufeatures: &CpuFeatures) -> Vec<MsrCategory> {
    let mut categories = Vec::new();

    // Long mode MSRs - architectural, always present in x86-64
    let mut long_mode_entries = read_msrs(&[
        ("IA32_EFER", MSR_EFER),
        ("IA32_STAR", MSR_STAR),
        ("IA32_LSTAR", MSR_LSTAR),
        ("IA32_CSTAR", MSR_CSTAR),
        ("IA32_FMASK", MSR_FMASK),
        ("IA32_FS_BASE", MSR_FSBASE),
        ("IA32_GS_BASE", MSR_GSBASE),
        ("IA32_KERNEL_GS_BASE", MSR_KERNELGSBASE),
    ]);
    // TSC_AUX requires RDTSCP support
    if let Some(entry) = read_msr_if("IA32_TSC_AUX", MSR_TSC_AUX, cpufeatures.has_rdtscp()) {
        long_mode_entries.push(entry);
    }
    categories.push(MsrCategory {
        name: "Long Mode / SYSCALL",
        entries: long_mode_entries,
    });

    // Core system MSRs - APIC_BASE is always present with APIC
    categories.push(MsrCategory {
        name: "System",
        entries: read_msrs(&[("IA32_APIC_BASE", MSR_IA32_APICBASE)]),
    });

    // Time-related MSRs
    let mut tsc_entries = read_msrs(&[("IA32_TSC", MSR_IA32_TSC)]);
    if let Some(entry) = read_msr_if("IA32_TSC_ADJUST", MSR_TSC_ADJUST, cpufeatures.has_tsc_adjust())
    {
        tsc_entries.push(entry);
    }
    categories.push(MsrCategory {
        name: "Time Stamp Counter",
        entries: tsc_entries,
    });

    // SYSENTER MSRs - architectural, always present
    categories.push(MsrCategory {
        name: "SYSENTER",
        entries: read_msrs(&[
            ("IA32_SYSENTER_CS", MSR_IA32_SYSENTER_CS),
            ("IA32_SYSENTER_ESP", MSR_IA32_SYSENTER_ESP),
            ("IA32_SYSENTER_EIP", MSR_IA32_SYSENTER_EIP),
        ]),
    });

    // Machine Check MSRs - only if MCE/MCA supported
    if cpufeatures.has_mce() && cpufeatures.has_mca() {
        categories.push(MsrCategory {
            name: "Machine Check",
            entries: read_msrs(&[
                ("IA32_MCG_CAP", MSR_MCG_CAP),
                ("IA32_MCG_STATUS", MSR_MCG_STATUS),
            ]),
        });
    }

    // MTRR MSRs - only if MTRR supported
    if cpufeatures.has_mtrr() {
        categories.push(MsrCategory {
            name: "MTRR",
            entries: read_msrs(&[
                ("IA32_MTRRCAP", MSR_MTRRCAP),
                ("IA32_MTRR_DEF_TYPE", MSR_MTRR_DEF_TYPE),
                ("IA32_MTRR_PHYSBASE0", MSR_MTRR_PHYSBASE0),
                ("IA32_MTRR_PHYSMASK0", MSR_MTRR_PHYSMASK0),
                ("IA32_MTRR_PHYSBASE1", MSR_MTRR_PHYSBASE1),
                ("IA32_MTRR_PHYSMASK1", MSR_MTRR_PHYSMASK1),
                ("IA32_MTRR_FIX64K_00000", MSR_MTRR_FIX64K_00000),
                ("IA32_MTRR_FIX16K_80000", MSR_MTRR_FIX16K_80000),
                ("IA32_MTRR_FIX16K_A0000", MSR_MTRR_FIX16K_A0000),
                ("IA32_MTRR_FIX4K_C0000", MSR_MTRR_FIX4K_C0000),
            ]),
        });
    }

    // PAT - only if PAT supported
    if cpufeatures.has_pat() {
        categories.push(MsrCategory {
            name: "PAT",
            entries: read_msrs(&[("IA32_PAT", MSR_PAT)]),
        });
    }

    categories
}

/// Pane wrapper for MSR state with scroll and search support
pub struct MsrPane {
    categories: Vec<MsrCategory>,
    scroll: ScrollHints,
    search: search::SearchState,
}

impl MsrPane {
    pub fn new(cpufeatures: &CpuFeatures) -> Self {
        Self {
            categories: read_all_msrs(cpufeatures),
            scroll: ScrollHints::default(),
            search: search::SearchState::default(),
        }
    }

    pub fn categories(&self) -> &[MsrCategory] {
        &self.categories
    }

    pub fn search_state(&self) -> &search::SearchState {
        &self.search
    }
}

impl Scrollable for MsrPane {
    fn scroll_hints_mut(&mut self) -> &mut ScrollHints {
        &mut self.scroll
    }
}

impl Searchable for MsrPane {
    fn search_state_mut(&mut self) -> &mut search::SearchState {
        &mut self.search
    }

    fn search_items(&self) -> Vec<(&str, u16)> {
        let mut items = Vec::new();
        let mut line: u16 = 0;

        for category in &self.categories {
            // Skip header line
            line += 1;

            for entry in &category.entries {
                items.push((entry.name, line));
                line += 1;
            }
            // Empty line between categories
            line += 1;
        }

        items
    }
}

impl Widget for &mut MsrPane {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let query = if self.search.last_query.is_empty() {
            None
        } else {
            Some(self.search.last_query.as_str())
        };

        let mut lines: Vec<Line> = Vec::new();
        let num_categories = self.categories.len();

        for (i, category) in self.categories.iter().enumerate() {
            // Category header
            lines.push(Line::styled(category.name, Style::default().bold()));

            for entry in &category.entries {
                let value_str = match entry.value {
                    Some(v) => format!("0x{:016x}", v),
                    None => "N/A".into(),
                };
                let suffix = format!(" (0x{:08X}) = {}", entry.address, value_str);
                lines.push(highlight_line(entry.name, &suffix, 24, query));
            }

            // Empty line between categories (but not after the last one)
            if i < num_categories - 1 {
                lines.push(Line::raw(""));
            }
        }

        let n_lines = lines.len();
        let paragraph = Paragraph::new(lines).scroll((self.scroll.y_offset, 0));

        paragraph.render(area, buf);

        self.scroll.update_from_render(n_lines, area.height);
    }
}
