use alloc::format;
use alloc::vec;
use core::arch::asm;
use core::fmt::LowerHex;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Paragraph, Widget};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::xcontrol::{XCr0, XCr0Flags};

use crate::cpuid::CpuidState;
use crate::pane::Scrollable;
use crate::pane::ScrollHints;

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

#[inline(always)]
pub fn enable_avx() {
    unsafe {
        // Enable OSXSAVE in CR4 to allow XCR0 access
        let mut cr4 = Cr4::read();
        cr4.insert(Cr4Flags::OSXSAVE);
        Cr4::write(cr4);

        // Enable AVX state (YMM registers) in XCR0
        let mut xcr0 = XCr0::read();
        xcr0.insert(XCr0Flags::X87);
        xcr0.insert(XCr0Flags::SSE);
        xcr0.insert(XCr0Flags::AVX);
        XCr0::write(xcr0);
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

/// YMM registers (256-bit) for AVX/AVX2
#[repr(C, align(32))]
pub struct YmmRegisters {
    pub ymm: [[u8; 32]; 16],
}

impl YmmRegisters {
    pub const fn new_zeroed() -> Self {
        Self { ymm: [[0; 32]; 16] }
    }
}

/// Read all 16 YMM registers using vmovdqu
#[inline(always)]
pub fn read_ymm_registers(out: &mut YmmRegisters) {
    unsafe {
        asm!(
            "vmovdqu [{ptr}], ymm0",
            "vmovdqu [{ptr} + 32], ymm1",
            "vmovdqu [{ptr} + 64], ymm2",
            "vmovdqu [{ptr} + 96], ymm3",
            "vmovdqu [{ptr} + 128], ymm4",
            "vmovdqu [{ptr} + 160], ymm5",
            "vmovdqu [{ptr} + 192], ymm6",
            "vmovdqu [{ptr} + 224], ymm7",
            "vmovdqu [{ptr} + 256], ymm8",
            "vmovdqu [{ptr} + 288], ymm9",
            "vmovdqu [{ptr} + 320], ymm10",
            "vmovdqu [{ptr} + 352], ymm11",
            "vmovdqu [{ptr} + 384], ymm12",
            "vmovdqu [{ptr} + 416], ymm13",
            "vmovdqu [{ptr} + 448], ymm14",
            "vmovdqu [{ptr} + 480], ymm15",
            ptr = in(reg) out as *mut YmmRegisters,
            options(nostack, preserves_flags),
        );
    }
}

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

#[derive(Default)]
pub struct FpuState {
    pub scroll: ScrollHints,
    pub has_avx2: bool,
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

impl FpuState {
    pub fn new(cpuid_state: &CpuidState) -> Self {
        enable_sse();
        write_xmm_values();

        let has_avx2 = cpuid_state.has_avx2();
        if has_avx2 {
            enable_avx();
        }

        Self {
            scroll: ScrollHints::default(),
            has_avx2,
        }
    }

    fn fxsave64(&self) -> FxSaveAligned {
        let mut area = FxSaveAligned::new_zeroed();
        fxsave64(&mut area);
        area
    }
}

impl Scrollable for FpuState {
    fn scroll_hints_mut(&mut self) -> &mut ScrollHints {
        &mut self.scroll
    }
}

impl Widget for &mut FpuState {
    fn render(self, area: Rect, buf: &mut Buffer) {
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
        if self.has_avx2 {
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
        let paragraph = Paragraph::new(Text::from(text)).scroll((self.scroll.y_offset, 0));
        paragraph.render(area, buf);

        self.scroll.update_from_render(n_lines, area.height);
    }
}
