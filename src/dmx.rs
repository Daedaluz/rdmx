#![allow(dead_code)]
use crate::serial;
use crate::serial::{ASYNC_LOW_LATENCY, ASYNC_SPD_CUST, tcsets};
use libc::{BOTHER, c_int, tcdrain, termios};
use std::ffi::CString;
use std::os::fd::AsFd;
use std::str::FromStr;
use clap_derive::ValueEnum;

fn spin_sleep(duration: std::time::Duration) {
    let start = std::time::Instant::now();
    while start.elapsed() < duration {
        // Busy-wait loop
    }
}

fn open(path: &str) -> Result<c_int, std::io::Error> {
    let c_path = CString::new(path)?;
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}

enum ResetMode {
    ResetTios2(libc::termios2),
    ResetSerial((serial::serial_struct, termios)),
}

pub(crate) struct Port {
    fd: c_int,
    reset: ResetMode,
}

impl Drop for Port {
    fn drop(&mut self) {
        match &self.reset {
            ResetMode::ResetTios2(oldtios) => {
                // Reset the termios settings to the old settings
                serial::tcsets2(self.fd, oldtios).expect("Failed to reset termios settings");
            }
            ResetMode::ResetSerial((oldserial, oldtermios)) => {
                // Reset the serial settings to the old settings
                tcsets(self.fd, oldtermios).expect("Failed to reset termios settings");
                serial::set_serial(self.fd, oldserial).expect("Failed to reset serial settings");
            }
        }
        // Close the file descriptor
        unsafe {
            libc::close(self.fd);
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Mode {
    Termios2,
    SetSerial,
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "TERMIOS2" => Ok(Mode::Termios2),
            "SETSERIAL" => Ok(Mode::SetSerial),
            _ => Err(format!("Invalid mode: {}", s)),
        }
    }
}

impl Port {
    pub fn fd(&self) -> c_int {
        self.fd
    }

    pub fn open(path: &str, mode: Mode) -> Result<Self, std::io::Error> {
        let fd = open(path)?;

        let reset = match mode {
            Mode::Termios2 => {
                // Get the old termios settings
                let oldtios = serial::tcgets2(fd)?;
                // Set the termios settings for DMX
                let mut tios2 = oldtios;
                tios2.c_cflag |= libc::CLOCAL | libc::CREAD; // Enable receiver and local mode
                tios2.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::CBAUD | libc::CBAUDEX); // 8N1 configuration
                tios2.c_cflag |= libc::CS8 | BOTHER; // 8 data bits
                tios2.c_iflag &= !(libc::IGNBRK
                    | libc::BRKINT
                    | libc::PARMRK
                    | libc::ISTRIP
                    | libc::INLCR
                    | libc::IGNCR
                    | libc::ICRNL);
                tios2.c_ospeed = 250000;
                tios2.c_ispeed = 250000;
                serial::tcsets2(fd, &tios2)?;
                ResetMode::ResetTios2(oldtios)
            }
            Mode::SetSerial => {
                // Set the termios settings for DMX
                let oldtios = serial::tcgets(fd)?;
                let mut tios = oldtios;
                tios.c_cflag |= libc::CLOCAL | libc::CREAD; // Enable receiver and local mode
                tios.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::CBAUD | libc::CBAUDEX); // 8N1 configuration
                tios.c_cflag |= libc::CS8 | libc::B38400; // 8 data bits
                tios.c_iflag &= !(libc::IGNBRK
                    | libc::BRKINT
                    | libc::PARMRK
                    | libc::ISTRIP
                    | libc::INLCR
                    | libc::IGNCR
                    | libc::ICRNL);
                tcsets(fd, &oldtios)?;

                // Get the serial settings
                let ss = serial::get_serial(fd)?;
                let divisor = ss.baud_base / 250000;
                if divisor == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Invalid baud rate divisor",
                    ));
                }
                // Set the serial settings for DMX
                let mut new_ss = ss;
                new_ss.baud_base = 250000;
                new_ss.flags = (ASYNC_SPD_CUST | ASYNC_LOW_LATENCY) as c_int;
                dbg!("Baud base={}, divisor={}", ss.baud_base, divisor);
                dbg!("Setting DMX baud rate to: {}", divisor);
                serial::set_serial(fd, &new_ss)?;
                ResetMode::ResetSerial((ss, oldtios))
            }
        };
        Ok(Port { fd, reset })
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, std::io::Error> {
        // Make sure everything is written to the port before sending next DMX frame
        unsafe {
            tcdrain(self.fd);
        }
        serial::set_brk(self.fd, 1)?;
        // sleep for 120 us - Break (BRK)
        spin_sleep(core::time::Duration::from_micros(120));
        serial::set_brk(self.fd, 0)?;
        // sleep for 12 us - mark after break (MAB)
        spin_sleep(core::time::Duration::from_micros(12));
        // Write the buffer to the DMX port - typically 513 bytes (512 channels + 1 start code)
        let res = unsafe { libc::write(self.fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if res < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(res as usize)
        }
    }
}

impl AsFd for Port {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        unsafe { std::os::fd::BorrowedFd::borrow_raw(self.fd) }
    }
}
