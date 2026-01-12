//! Model Specific Register (MSR) reading and display

use alloc::vec::Vec;

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

// MSR addresses from QEMU cpu.h
const MSR_EFER: u32 = 0xC000_0080;
const MSR_STAR: u32 = 0xC000_0081;
const MSR_LSTAR: u32 = 0xC000_0082;
const MSR_CSTAR: u32 = 0xC000_0083;
const MSR_FMASK: u32 = 0xC000_0084;
const MSR_FSBASE: u32 = 0xC000_0100;
const MSR_GSBASE: u32 = 0xC000_0101;
const MSR_KERNELGSBASE: u32 = 0xC000_0102;
const MSR_TSC_AUX: u32 = 0xC000_0103;

const MSR_IA32_TSC: u32 = 0x10;
const MSR_IA32_APICBASE: u32 = 0x1B;
const MSR_IA32_FEATURE_CONTROL: u32 = 0x3A;
const MSR_TSC_ADJUST: u32 = 0x3B;
const MSR_IA32_SPEC_CTRL: u32 = 0x48;
const MSR_IA32_MISC_ENABLE: u32 = 0x1A0;

const MSR_IA32_SYSENTER_CS: u32 = 0x174;
const MSR_IA32_SYSENTER_ESP: u32 = 0x175;
const MSR_IA32_SYSENTER_EIP: u32 = 0x176;

const MSR_MCG_CAP: u32 = 0x179;
const MSR_MCG_STATUS: u32 = 0x17A;

const MSR_MTRRCAP: u32 = 0xFE;
const MSR_MTRR_DEF_TYPE: u32 = 0x2FF;
const MSR_PAT: u32 = 0x277;

const MSR_MTRR_PHYSBASE0: u32 = 0x200;
const MSR_MTRR_PHYSMASK0: u32 = 0x201;
const MSR_MTRR_PHYSBASE1: u32 = 0x202;
const MSR_MTRR_PHYSMASK1: u32 = 0x203;

const MSR_MTRR_FIX64K_00000: u32 = 0x250;
const MSR_MTRR_FIX16K_80000: u32 = 0x258;
const MSR_MTRR_FIX16K_A0000: u32 = 0x259;
const MSR_MTRR_FIX4K_C0000: u32 = 0x268;

/// Build all MSR categories with current values
/// Only includes MSRs that are safe to read on x86-64 long mode
pub fn read_all_msrs() -> Vec<MsrCategory> {
    let mut categories = Vec::new();

    // EFER and long mode - architectural in long mode
    categories.push(MsrCategory {
        name: "Long Mode / SYSCALL",
        entries: read_msrs(&[
            ("IA32_EFER", MSR_EFER),
            ("IA32_STAR", MSR_STAR),
            ("IA32_LSTAR", MSR_LSTAR),
            ("IA32_CSTAR", MSR_CSTAR),
            ("IA32_FMASK", MSR_FMASK),
            ("IA32_FS_BASE", MSR_FSBASE),
            ("IA32_GS_BASE", MSR_GSBASE),
            ("IA32_KERNEL_GS_BASE", MSR_KERNELGSBASE),
            ("IA32_TSC_AUX", MSR_TSC_AUX),
        ]),
    });

    // Core system MSRs
    categories.push(MsrCategory {
        name: "System",
        entries: read_msrs(&[
            ("IA32_APIC_BASE", MSR_IA32_APICBASE),
            ("IA32_FEATURE_CONTROL", MSR_IA32_FEATURE_CONTROL),
            ("IA32_MISC_ENABLE", MSR_IA32_MISC_ENABLE),
            ("IA32_SPEC_CTRL", MSR_IA32_SPEC_CTRL),
        ]),
    });

    // Time-related MSRs
    categories.push(MsrCategory {
        name: "Time Stamp Counter",
        entries: read_msrs(&[
            ("IA32_TSC", MSR_IA32_TSC),
            ("IA32_TSC_ADJUST", MSR_TSC_ADJUST),
        ]),
    });

    // SYSENTER MSRs - architectural
    categories.push(MsrCategory {
        name: "SYSENTER",
        entries: read_msrs(&[
            ("IA32_SYSENTER_CS", MSR_IA32_SYSENTER_CS),
            ("IA32_SYSENTER_ESP", MSR_IA32_SYSENTER_ESP),
            ("IA32_SYSENTER_EIP", MSR_IA32_SYSENTER_EIP),
        ]),
    });

    // Machine Check MSRs
    categories.push(MsrCategory {
        name: "Machine Check",
        entries: read_msrs(&[
            ("IA32_MCG_CAP", MSR_MCG_CAP),
            ("IA32_MCG_STATUS", MSR_MCG_STATUS),
        ]),
    });

    // MTRR MSRs - Memory Type Range Registers
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

    // PAT - Page Attribute Table
    categories.push(MsrCategory {
        name: "PAT",
        entries: read_msrs(&[("IA32_PAT", MSR_PAT)]),
    });

    categories
}
