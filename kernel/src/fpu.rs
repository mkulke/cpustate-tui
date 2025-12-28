use core::arch::asm;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

#[inline(always)]
pub fn enable_sse() {
    unsafe {
        // CR0:
        //  - clear EM (no x87 emulation) or SSE/x87 instructions can #UD
        //  - set MP (monitor coprocessor) ÔÇö conventional when using TS/#NM
        //  - clear TS (avoid #NM until you implement lazy switching)
        let mut cr0 = Cr0::read();
        cr0.remove(Cr0Flags::EMULATE_COPROCESSOR); // EM=0
        cr0.insert(Cr0Flags::MONITOR_COPROCESSOR); // MP=1
        cr0.remove(Cr0Flags::TASK_SWITCHED); // TS=0
        Cr0::write(cr0);

        // CR4:
        //  - OSFXSR enables FXSAVE/FXRSTOR and SSE instructions
        //  - OSXMMEXCPT enables SIMD FP exceptions (#XM)
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSFXSR);
        cr4.insert(Cr4Flags::OSXMMEXCPT_ENABLE);
        Cr4::write(cr4);
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FxSaveArea {
    // 0x00
    pub fcw: u16,
    pub fsw: u16,
    pub ftw: u8,
    pub _r1: u8,
    pub fop: u16,
    pub fip: u64, // in 64-bit mode: RIP of last x87 instruction
    pub fdp: u64, // in 64-bit mode: RDP of last x87 mem operand
    // 0x20
    pub mxcsr: u32,
    pub mxcsr_mask: u32,
    // 0x28
    pub st_mm: [[u8; 16]; 8], // x87/MMX regs in 80-bit format padded to 16 bytes
    pub xmm: [[u8; 16]; 16],  // XMM0..XMM15
    pub _rest: [u8; 96],      // padding / reserved to reach 512 bytes
}

#[repr(C, align(16))]
pub struct FxSaveAligned(pub FxSaveArea);

impl FxSaveAligned {
    pub const fn new_zeroed() -> Self {
        Self(FxSaveArea {
            fcw: 0,
            fsw: 0,
            ftw: 0,
            _r1: 0,
            fop: 0,
            fip: 0,
            fdp: 0,
            mxcsr: 0,
            mxcsr_mask: 0,
            st_mm: [[0; 16]; 8],
            xmm: [[0; 16]; 16],
            _rest: [0; 96],
        })
    }
}

#[inline(always)]
pub fn fxsave64(out: &mut FxSaveAligned) {
    unsafe {
        asm!(
            "fxsave64 [{}]",
            in(reg) out as *mut FxSaveAligned,
            options(nostack, preserves_flags),
        );
    }
}

#[inline(always)]
pub fn set_xmm0_bytes(v: &[u8; 16]) {
    unsafe {
        asm!(
            "movdqu xmm0, [{p}]",
            p = in(reg) v.as_ptr(),
            options(nostack, preserves_flags),
        );
    }
}

#[inline(always)]
pub fn set_xmm15_bytes(v: &[u8; 16]) {
    unsafe {
        asm!(
            "movdqu xmm15, [{p}]",
            p = in(reg) v.as_ptr(),
            options(nostack, preserves_flags),
        );
    }
}
