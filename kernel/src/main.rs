#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(abi_x86_interrupt)]

extern crate alloc;

use alloc::boxed::Box;
use app::{App, Pane};
use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{entry_point, BootInfo};
use core::fmt::Write;
use core::sync::atomic::Ordering;
use uart_16550::SerialPort;
use view::View;
use x86_64::instructions::hlt;

mod app;
mod fpu;
mod interrupts;
mod ioapic;
mod irq_mutex;
mod lapic;
mod memory;
mod ratatui_backend;
mod serial;
mod view;

static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) -> ! {
    use x86_64::instructions::{nop, port::Port};

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }

    loop {
        nop();
    }
}

fn main_loop(port: &mut SerialPort) -> ! {
    // let backend = ratatui_backend::SerialAnsiBackend::new(port, 80, 24);
    // let mut terminal = Terminal::new(backend).unwrap();

    let mut app = App::new();
    let mut view = View::new(port);

    // initial draw
    view.draw(&app);

    let mut needs_redraw = false;

    let mut abort = false;
    while !abort {
        hlt();

        let n = interrupts::PRINT_EVENTS.swap(0, Ordering::AcqRel);
        for _ in 0..n {
            app.tick();
            needs_redraw = true;
        }

        serial::RX_QUEUE.with(|queue| {
            let mut queue = queue.borrow_mut();
            let (_prod, mut cons) = queue.split();

            while let Some(byte) = cons.dequeue() {
                match byte {
                    b'q' => {
                        abort = true;
                    }
                    b'c' => {
                        app.set_pane(Pane::Cpuid);
                        needs_redraw = true;
                    }
                    b'f' => {
                        app.set_pane(Pane::Fpu);
                        needs_redraw = true;
                    }
                    b'x' => {
                        app.set_pane(Pane::Xsave);
                        needs_redraw = true;
                    }
                    b'j' => {
                        view.scroll_down(&mut app);
                        needs_redraw = true;
                    }
                    b'k' => {
                        view.scroll_up(&mut app);
                        needs_redraw = true;
                    }
                    _ => {}
                }
            }
        });

        if needs_redraw {
            view.draw(&app);
            needs_redraw = false;
        }
    }

    exit_qemu(QemuExitCode::Success);
}

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    let mut port = serial::port();
    writeln!(port, "Entered kernel with boot info: {boot_info:#?}").unwrap();

    let mappings = memory::init(boot_info);

    let x = Box::new(41);
    writeln!(port, "heap_value at {:p}", x).unwrap();

    interrupts::init(&mappings);
    writeln!(port, "done").unwrap();

    main_loop(&mut port);
}

/// This function is called on panic.
#[panic_handler]
#[cfg(not(test))]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let _ = writeln!(serial::port(), "PANIC: {info}");
    exit_qemu(QemuExitCode::Failed);
}
