use core::arch::x86_64::_rdtsc;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder, TimerDivide, TimerMode};

use crate::cpuid;

pub const TIMER_VECTOR: u8 = 0x20;
pub const ERROR_VECTOR: u8 = 0x21;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

const APIC_TIMER_DIVIDE: TimerDivide = TimerDivide::Div16;

/// Target timer frequency in Hz (ticks per second)
pub const TARGET_TIMER_HZ: u64 = 100;

/// Fallback initial count if calibration fails
const FALLBACK_TIMER_INITIAL: u32 = 1_000_000;

pub struct Lapic(LocalApic);

/// Calibrated LAPIC timer frequency in Hz
static mut LAPIC_TIMER_FREQ_HZ: Option<u64> = None;

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
        let initial_count = self.calibrate_timer().unwrap_or(FALLBACK_TIMER_INITIAL);

        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::Periodic);
            self.0.set_timer_initial(initial_count);
            self.0.enable_timer();
        }
    }

    /// Calibrate the LAPIC timer using TSC as reference.
    /// Returns the initial count value needed for TARGET_TIMER_HZ.
    fn calibrate_timer(&mut self) -> Option<u32> {
        let tsc_freq = cpuid::tsc_frequency()?;

        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::OneShot);
            self.0.disable_timer();

            // Start with max count
            let start_count: u32 = 0xFFFF_FFFF;
            self.0.set_timer_initial(start_count);

            // Measure TSC cycles for ~10ms worth of TSC ticks
            let calibration_tsc_ticks = tsc_freq / 100; // 10ms
            let tsc_start = _rdtsc();
            self.0.enable_timer();

            // Spin until enough TSC cycles have passed
            while _rdtsc() - tsc_start < calibration_tsc_ticks {}

            self.0.disable_timer();
            let end_count = self.0.timer_current();
            let elapsed_lapic_ticks = start_count - end_count;

            // Calculate LAPIC timer frequency
            // elapsed_lapic_ticks happened in (calibration_tsc_ticks / tsc_freq) seconds
            // lapic_freq = elapsed_lapic_ticks / (calibration_tsc_ticks / tsc_freq)
            //            = elapsed_lapic_ticks * tsc_freq / calibration_tsc_ticks
            let lapic_freq = (elapsed_lapic_ticks as u64 * tsc_freq) / calibration_tsc_ticks;

            LAPIC_TIMER_FREQ_HZ = Some(lapic_freq);

            // Calculate initial count for target frequency
            let initial_count = (lapic_freq / TARGET_TIMER_HZ) as u32;
            Some(initial_count)
        }
    }

    pub fn eoi(&mut self) {
        unsafe {
            self.0.end_of_interrupt();
        }
    }
}

/// Returns the calibrated LAPIC timer frequency in Hz, if available.
pub fn lapic_timer_freq_hz() -> Option<u64> {
    unsafe { LAPIC_TIMER_FREQ_HZ }
}
