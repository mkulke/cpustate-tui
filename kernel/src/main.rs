#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(abi_x86_interrupt)]

extern crate alloc;

use app::App;
use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{entry_point, BootInfo};
use core::fmt::Write;

mod app;
mod cpuid;
mod fpu;
mod input;
mod interrupts;
mod ioapic;
mod irq_mutex;
mod lapic;
mod memory;
#[cfg(feature = "msr")]
mod msr;
mod pane;
mod qemu;
mod ratatui_backend;
mod serial;
mod timer;

static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let mut port = serial::port();
    writeln!(port, "boot info: {boot_info:#?}").unwrap();

    let mappings = memory::init(boot_info);
    interrupts::init(&mappings);

    writeln!(port, "init done").unwrap();

    let mut app = App::new();
    app.run();
}

#[panic_handler]
#[cfg(not(test))]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(serial::port(), "PANIC: {info}");
    qemu::exit(qemu::QemuExitCode::Failed);
}
