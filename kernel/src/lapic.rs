// use core::ptr::addr_of_mut;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder, TimerDivide, TimerMode};

pub const TIMER_VECTOR: u8 = 0x20;
pub const ERROR_VECTOR: u8 = 0x21;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

const APIC_TIMER_DIVIDE: TimerDivide = TimerDivide::Div16;
const APIC_TIMER_INITIAL: u32 = 1_000_000;

// static mut LAPIC: Lapic = Lapic::new();

// pub fn lapic() -> &'static mut Lapic {
//     unsafe { (*addr_of_mut!(LAPIC)).as_mut() }
// }

pub struct Lapic(LocalApic);

impl Lapic {
    pub fn new() -> Self {
        let base: u64;
        unsafe {
            base = xapic_base();
        }
        let lapic = LocalApicBuilder::new()
            .timer_vector(TIMER_VECTOR as usize)
            .error_vector(ERROR_VECTOR as usize)
            .spurious_vector(SPURIOUS_VECTOR as usize)
            .set_xapic_base(base)
            .build()
            .unwrap();
        Self(lapic)
    }

    pub fn enable(&mut self) {
        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::Periodic);
            self.0.set_timer_initial(APIC_TIMER_INITIAL);
            self.0.enable_timer();
        }
    }

    pub fn eoi(&mut self) {
        unsafe {
            self.0.end_of_interrupt();
        }
    }
}
