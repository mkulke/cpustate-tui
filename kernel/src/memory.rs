use crate::ioapic;
use bootloader_api::BootInfo;
use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use linked_list_allocator::LockedHeap;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

pub const HEAP_START: usize = 0x_1234_abcd_0000;
// pub const HEAP_SIZE: usize = 100 * 1024; // 100 KiB
pub const HEAP_SIZE: usize = 512 * 1024; // 512 KiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

pub struct UninitPageTableManager;

pub struct PageTableManager {
    mapper: OffsetPageTable<'static>,
}

impl PageTableManager {
    pub fn mapper(&mut self) -> &mut OffsetPageTable<'static> {
        &mut self.mapper
    }
}

impl UninitPageTableManager {
    pub const fn new() -> Self {
        Self
    }

    /// Consume the Uninitialized manager and returns an Initialized one.
    /// Can only be called once because it moves self.
    pub unsafe fn init(self, phys_offset: VirtAddr) -> PageTableManager {
        let (level_4_table_frame, _) = Cr3::read();

        let phys = level_4_table_frame.start_address();
        let virt = phys_offset + phys.as_u64();
        let page_table_ptr: *mut PageTable = virt.as_mut_ptr();
        let level_4_table;
        let mapper;
        unsafe {
            level_4_table = &mut *page_table_ptr;
            mapper = OffsetPageTable::new(level_4_table, phys_offset);
        }

        PageTableManager { mapper }
    }
}

fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_alloc: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + (HEAP_SIZE as u64) - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    for page in page_range {
        let frame = frame_alloc
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe { mapper.map_to(page, frame, flags, frame_alloc)?.flush() };
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    Ok(())
}

pub struct Mappings {
    ioapic_base: VirtAddr,
}

impl Mappings {
    pub fn ioapic_base(&self) -> VirtAddr {
        self.ioapic_base
    }
}

pub fn init(boot_info: &'static mut BootInfo) -> Mappings {
    let phys_mem_offset = boot_info
        .physical_memory_offset
        .into_option()
        .map(VirtAddr::new)
        .expect("physical memory offset not provided");

    let mut manager;
    let mut frame_alloc;
    unsafe {
        manager = UninitPageTableManager::new().init(phys_mem_offset);
        frame_alloc = BootInfoFrameAllocator::init(&boot_info.memory_regions);
    }
    let mapper = manager.mapper();
    init_heap(mapper, &mut frame_alloc).expect("heap initialization failed");

    let ioapic_base = ioapic::map_ioapic(mapper, &mut frame_alloc).expect("ioapic mapping failed");

    Mappings { ioapic_base }
}

/// A FrameAllocator that returns usable frames from the bootloader's memory map.
struct BootInfoFrameAllocator {
    regions: &'static MemoryRegions,
    next: usize,
}

impl BootInfoFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    unsafe fn init(regions: &'static MemoryRegions) -> Self {
        Self { regions, next: 0 }
    }

    /// Returns an iterator over the usable frames specified in the memory map.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // get usable regions from memory map
        let regions = self.regions.iter();
        let usable_regions = regions.filter(|r| r.kind == MemoryRegionKind::Usable);
        // map each region to its address range
        let addr_ranges = usable_regions.map(|r| r.start..r.end);
        // transform to an iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
        // create `PhysFrame` types from the start addresses
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
