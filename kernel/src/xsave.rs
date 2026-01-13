use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};

use crate::cpuid::CpuidState;

// XSAVE State merely references CPUID state
pub struct XsaveState {
    leaf_0x1_0: [u32; 4],
    leaf_0xd_0: [u32; 4],
    leaf_0xd_1: [u32; 4],
    has_xsave: bool,
}

impl XsaveState {
    pub fn new(cpuid_state: &CpuidState) -> Self {
        let leaf_0x1_0 = cpuid_state.leaf(0x1, 0);
        let leaf_0xd_0 = cpuid_state.leaf(0xd, 0);
        let leaf_0xd_1 = cpuid_state.leaf(0xd, 1);
        Self {
            leaf_0x1_0,
            leaf_0xd_0,
            leaf_0xd_1,
            has_xsave: cpuid_state.has_xsave(),
        }
    }
}

impl Widget for &XsaveState {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // let state = &self.cpuid_state;
        let line_1 = format!("Leaf 0x1 reports XSAVE: {}", self.has_xsave);
        let [eax, ebx, ecx, edx] = self.leaf_0x1_0;
        let line_2 = format!(
            "Leaf 0x1:0 -> EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x}",
            eax, ebx, ecx, edx
        );
        let [eax, ebx, ecx, edx] = self.leaf_0xd_0;
        let line_3 = format!(
            "Leaf 0xD:0 -> EAX={:08x} EBX={:08x} ECX={:08x} EDX={:08x}",
            eax, ebx, ecx, edx
        );
        let [eax, ..] = self.leaf_0xd_1;
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
}
