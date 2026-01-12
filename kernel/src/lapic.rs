use core::arch::x86_64::_rdtsc;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder, TimerDivide, TimerMode};
use x86_64::instructions::port::Port;

use crate::cpuid;

pub const TIMER_VECTOR: u8 = 0x20;
pub const ERROR_VECTOR: u8 = 0x21;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

const APIC_TIMER_DIVIDE: TimerDivide = TimerDivide::Div16;

/// Target timer frequency in Hz (ticks per second)
pub const TARGET_TIMER_HZ: u64 = 100;

/// Fallback initial count if calibration fails
const FALLBACK_TIMER_INITIAL: u32 = 1_000_000;

/// PIT frequency in Hz (1.193182 MHz)
const PIT_FREQUENCY: u64 = 1_193_182;

/// PIT I/O ports
const PIT_CHANNEL2_DATA: u16 = 0x42;
const PIT_COMMAND: u16 = 0x43;
const PIT_CONTROL: u16 = 0x61;

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
        let initial_count = self
            .calibrate_with_tsc()
            .or_else(|| self.calibrate_with_pit())
            .unwrap_or(FALLBACK_TIMER_INITIAL);

        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::Periodic);
            self.0.set_timer_initial(initial_count);
            self.0.enable_timer();
        }
    }

    /// Calibrate the LAPIC timer using TSC as reference.
    fn calibrate_with_tsc(&mut self) -> Option<u32> {
        let tsc_freq = cpuid::tsc_frequency()?;

        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::OneShot);
            self.0.disable_timer();

            let start_count: u32 = 0xFFFF_FFFF;
            self.0.set_timer_initial(start_count);

            // Measure for ~10ms worth of TSC ticks
            let calibration_tsc_ticks = tsc_freq / 100;
            let tsc_start = _rdtsc();
            self.0.enable_timer();

            while _rdtsc() - tsc_start < calibration_tsc_ticks {}

            self.0.disable_timer();
            let end_count = self.0.timer_current();
            let elapsed_lapic_ticks = start_count - end_count;

            let lapic_freq = (elapsed_lapic_ticks as u64 * tsc_freq) / calibration_tsc_ticks;
            LAPIC_TIMER_FREQ_HZ = Some(lapic_freq);

            Some((lapic_freq / TARGET_TIMER_HZ) as u32)
        }
    }

    /// Calibrate the LAPIC timer using PIT channel 2 as reference.
    fn calibrate_with_pit(&mut self) -> Option<u32> {
        unsafe {
            self.0.enable();
            self.0.set_timer_divide(APIC_TIMER_DIVIDE);
            self.0.set_timer_mode(TimerMode::OneShot);
            self.0.disable_timer();

            // Set up PIT channel 2 for ~10ms (11932 ticks at 1.193182 MHz)
            let pit_ticks: u16 = (PIT_FREQUENCY / 100) as u16; // ~10ms

            let mut control_port = Port::<u8>::new(PIT_CONTROL);
            let mut command_port = Port::<u8>::new(PIT_COMMAND);
            let mut channel2_port = Port::<u8>::new(PIT_CHANNEL2_DATA);

            // Disable speaker, enable PIT channel 2 gate
            let control = control_port.read();
            control_port.write((control & 0xFC) | 0x01);

            // Program PIT channel 2: mode 0 (interrupt on terminal count), binary
            // 0xB0 = channel 2, lobyte/hibyte, mode 0, binary
            command_port.write(0xB0);

            // Write count (low byte first, then high byte)
            channel2_port.write((pit_ticks & 0xFF) as u8);
            channel2_port.write((pit_ticks >> 8) as u8);

            // Start LAPIC timer with max count
            let start_count: u32 = 0xFFFF_FFFF;
            self.0.set_timer_initial(start_count);

            // Reset PIT gate to start countdown
            let control = control_port.read();
            control_port.write(control & 0xFE); // Gate low
            control_port.write(control | 0x01); // Gate high - starts countdown

            self.0.enable_timer();

            // Wait for PIT channel 2 output to go high (bit 5 of port 0x61)
            while (control_port.read() & 0x20) == 0 {}

            self.0.disable_timer();
            let end_count = self.0.timer_current();
            let elapsed_lapic_ticks = start_count - end_count;

            // Calculate LAPIC frequency: elapsed_ticks / (pit_ticks / PIT_FREQUENCY)
            let lapic_freq = (elapsed_lapic_ticks as u64 * PIT_FREQUENCY) / pit_ticks as u64;
            LAPIC_TIMER_FREQ_HZ = Some(lapic_freq);

            Some((lapic_freq / TARGET_TIMER_HZ) as u32)
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
