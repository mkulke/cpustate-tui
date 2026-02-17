use heapless::spsc::Queue;
use spin::Mutex;
pub use uart_16550::SerialPort;
use x86::io::inb;

const COM1_BASE: u16 = 0x3F8;
const COM1_DATA: u16 = COM1_BASE;
const COM1_LSR: u16 = COM1_BASE + 5;
pub const COM1_IRQ: u8 = 0x04;

pub static RX_QUEUE: Mutex<Queue<u8, 256>> = Mutex::new(Queue::new());

pub fn port() -> SerialPort {
    let mut port = unsafe { uart_16550::SerialPort::new(0x3F8) };
    port.init();
    port
}

pub fn uart_rx_ready() -> bool {
    unsafe { (inb(COM1_LSR) & 0x01) != 0 }
}

pub fn uart_read_byte() -> u8 {
    unsafe { inb(COM1_DATA) }
}
