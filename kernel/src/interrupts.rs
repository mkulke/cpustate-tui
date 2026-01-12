use crate::ioapic::{self, COM1_VECTOR};
use crate::lapic::{Lapic, ERROR_VECTOR, SPURIOUS_VECTOR, TARGET_TIMER_HZ, TIMER_VECTOR};
use crate::memory;
use crate::serial;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Once;
use x86_64::instructions::interrupts;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

static IDT: Once<InterruptDescriptorTable> = Once::new();
static TICK_COUNT: AtomicUsize = AtomicUsize::new(0);
pub static PRINT_EVENTS: AtomicUsize = AtomicUsize::new(0);
pub static SECOND_EVENTS: AtomicUsize = AtomicUsize::new(0);

/// Ticks per color change (1 second at TARGET_TIMER_HZ)
const TICKS_PER_EVENT: usize = TARGET_TIMER_HZ as usize;

/// Ticks per second
const TICKS_PER_SECOND: usize = TARGET_TIMER_HZ as usize;

/// Returns the current tick count since boot
pub fn tick_count() -> usize {
    TICK_COUNT.load(Ordering::Relaxed)
}

fn lapic() -> Lapic {
    Lapic::new()
}

extern "x86-interrupt" fn timer_interrupt_handler(_sf: InterruptStackFrame) {
    let ticks = TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

    if ticks.is_multiple_of(TICKS_PER_SECOND) {
        SECOND_EVENTS.fetch_add(1, Ordering::Relaxed);
    }

    if ticks.is_multiple_of(TICKS_PER_EVENT) {
        PRINT_EVENTS.fetch_add(1, Ordering::Relaxed);
    }

    lapic().eoi();
}

extern "x86-interrupt" fn com1_interrupt_handler(_stack_frame: InterruptStackFrame) {
    serial::RX_QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let (mut prod, _cons) = queue.split();
        while serial::uart_rx_ready() {
            let byte = serial::uart_read_byte();
            // we want to ignore overflow here
            _ = prod.enqueue(byte);
        }
    });
    lapic().eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(_sf: InterruptStackFrame) {
    lapic().eoi();
}

extern "x86-interrupt" fn spurious_interrupt_handler(_sf: InterruptStackFrame) {
    lapic().eoi();
}

pub fn init(mappings: &memory::Mappings) {
    let idt = IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt[TIMER_VECTOR].set_handler_fn(timer_interrupt_handler);
        idt[ERROR_VECTOR].set_handler_fn(error_interrupt_handler);
        idt[SPURIOUS_VECTOR].set_handler_fn(spurious_interrupt_handler);
        idt[COM1_VECTOR].set_handler_fn(com1_interrupt_handler);

        idt
    });
    idt.load();
    let mut lapic = Lapic::new();
    lapic.enable();

    ioapic::disable_pic();
    ioapic::init(mappings.ioapic_base());
    interrupts::enable();
}
