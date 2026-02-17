use pic8259::ChainedPics;
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, Mapper, Page, PageTableFlags as Flags, PhysFrame, Size4KiB,
        mapper::MapToError,
    },
};

const IOAPIC_PHYS: u64 = 0xFEC0_0000;
const IOAPIC_VIRT: u64 = 0xFFFF_FF00_FEC0_0000;

const PIC_1_OFFSET: u8 = 0x20;
const PIC_2_OFFSET: u8 = 0x28;

pub fn disable_pic() {
    unsafe {
        let mut pics = ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET);
        // Mask all interrupts (0xFF = all bits set = all masked)
        pics.write_masks(0xFF, 0xFF);
    }
}

pub fn map_ioapic(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_alloc: &mut impl FrameAllocator<Size4KiB>,
) -> Result<VirtAddr, MapToError<Size4KiB>> {
    let phys = PhysAddr::new(IOAPIC_PHYS);
    let virt = VirtAddr::new(IOAPIC_VIRT);

    // IOAPIC base is 4KiB-aligned; if not, you'd need to handle offsets.
    assert_eq!(IOAPIC_PHYS & 0xfff, 0);

    let page = Page::<Size4KiB>::containing_address(virt);
    let frame = PhysFrame::<Size4KiB>::containing_address(phys);

    let flags =
        Flags::PRESENT | Flags::WRITABLE | Flags::NO_CACHE | Flags::NO_EXECUTE | Flags::GLOBAL;

    unsafe {
        mapper.map_to(page, frame, flags, frame_alloc)?.flush();
    }

    Ok(virt)
}

use crate::serial::COM1_IRQ;
use x2apic::ioapic::{self, IrqFlags, IrqMode, RedirectionTableEntry};

// const IOAPIC_BASE: u64 = 0xFEC0_0000;
const VECTOR_OFFSET: u8 = 0x20;
pub const COM1_VECTOR: u8 = VECTOR_OFFSET + COM1_IRQ;
const DEST_CPU: u8 = 0;

pub fn init(ioapic_base: VirtAddr) {
    let mut ioapic;
    unsafe {
        ioapic = ioapic::IoApic::new(ioapic_base.as_u64());
        ioapic.init(VECTOR_OFFSET);
    }

    let mut entry = RedirectionTableEntry::default();
    entry.set_mode(IrqMode::Fixed);
    entry.set_flags(IrqFlags::empty());
    entry.set_dest(DEST_CPU);
    entry.set_vector(COM1_VECTOR);

    unsafe {
        ioapic.set_table_entry(COM1_IRQ, entry);
        ioapic.enable_irq(COM1_IRQ);
    }
}
