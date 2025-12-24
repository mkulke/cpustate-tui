#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(abi_x86_interrupt)]

extern crate alloc;

use alloc::boxed::Box;
use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{entry_point, BootInfo};
use core::fmt::Write;
use core::sync::atomic::Ordering;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use uart_16550::SerialPort;
use x86_64::instructions::hlt;

mod app_state;
mod interrupts;
mod ioapic;
mod irq_mutex;
mod lapic;
mod memory;
mod serial;
mod ratatui_backend;

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

fn ui(f: &mut Frame<'_>, app: &app_state::AppState) {
    let area = f.area();

    let block = Block::default()
        .title("Serial ANSI Demo")
        .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [_, content, _] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(inner);

    let p = Paragraph::new(Line::styled(
        "Hello from Ratatui!",
        Style::default().fg(app.color()),
    ))
    .centered();

    f.render_widget(p, content);
}

fn main_loop(port: &mut SerialPort) -> ! {
    let backend = ratatui_backend::SerialAnsiBackend::new(port, 80, 25);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut app = app_state::AppState::new();

    // initial draw
    terminal.draw(|f| ui(f, &app)).unwrap();

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

            while let Some(byte) = cons.dequeue() && byte == b'q' {
                abort = true;
            }
        });

        if needs_redraw {
            terminal.draw(|f| ui(f, &app)).unwrap();
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
