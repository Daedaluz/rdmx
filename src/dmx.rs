use crate::serial;
use crate::serial::{ASYNC_LOW_LATENCY, ASYNC_SPD_CUST, tcsets};
use clap_derive::ValueEnum;
use libc::{
    B38400, BOTHER, BRKINT, CBAUD, CBAUDEX, CLOCAL, CREAD, CRTSCTS, CS8, CSIZE, CSTOPB, ECHO,
    ECHOE, ECHONL, ICANON, ICRNL, IGNBRK, IGNCR, INLCR, ISIG, ISTRIP, IXANY, IXOFF, IXON, ONLCR,
    OPOST, PARENB, PARMRK, c_int, tcdrain, termios,
};
use std::ffi::CString;
use std::os::fd::AsFd;
use std::str::FromStr;

// Good reference for dmx packet / timing
// https://support.etcconnect.com/ETC/FAQ/DMX_Speed

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
                _ = serial::tcsets2(self.fd, oldtios).or_else(|e| {
                    println!("Failed to reset termios settings: {}", e);
                    Err(e)
                });
            }
            ResetMode::ResetSerial((oldserial, oldtermios)) => {
                // Reset the serial settings to the old settings
                _ = tcsets(self.fd, oldtermios).or_else(|e| {
                    println!("Failed to reset termios settings");
                    Err(e)
                });
                _ = serial::set_serial(self.fd, oldserial).or_else(|e| {
                    println!("Failed to reset serial settings");
                    Err(e)
                });
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
            "SET-SERIAL" => Ok(Mode::SetSerial),
            _ => Err(format!("Invalid mode: {}", s)),
        }
    }
}

impl Port {
    fn configure_termios2(fd: c_int) -> Result<ResetMode, std::io::Error> {
        // Get the old termios settings
        let oldtios = serial::tcgets2(fd)?;
        // Set the termios settings for DMX
        let mut tios2 = oldtios;
        tios2.c_cflag &= !(PARENB | CSIZE | CRTSCTS | CBAUD);
        tios2.c_cflag |= CSTOPB | CS8 | CREAD | CLOCAL | CBAUDEX;
        tios2.c_lflag &= !(ICANON | ECHO | ECHOE | ECHONL | ISIG);
        tios2.c_iflag &= !(IXON | IXOFF | IXANY);
        tios2.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL);
        tios2.c_oflag &= !(OPOST | ONLCR);
        tios2.c_ospeed = 250000;
        tios2.c_ispeed = 250000;
        serial::tcsets2(fd, &tios2)?;
        Ok(ResetMode::ResetTios2(oldtios))
    }

    fn configure_set_serial(fd: c_int) -> Result<ResetMode, std::io::Error> {
        // Set the termios settings for DMX
        // The baud should be set to B38400, for special interpretation when configuring
        // the serial devices divisor
        let oldtios = serial::tcgets(fd)?;
        let mut tios = oldtios;
        tios.c_cflag &= !(PARENB | CSIZE | CRTSCTS | CBAUD);
        tios.c_cflag |= CSTOPB | CS8 | CREAD | CLOCAL | B38400;
        tios.c_lflag &= !(ICANON | ECHO | ECHOE | ECHONL | ISIG);
        tios.c_iflag &= !(IXON | IXOFF | IXANY);
        tios.c_iflag &= !(IGNBRK | BRKINT | PARMRK | ISTRIP | INLCR | IGNCR | ICRNL);
        tios.c_oflag &= !(OPOST | ONLCR);
        tcsets(fd, &oldtios)?;

        // Get the serial settings
        let ss = serial::get_serial(fd)?;
        let divisor = ss.baud_base / 250000;
        if divisor == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid baud rate divisor (0)",
            ));
        }
        println!("Baud base: {}", ss.baud_base);
        println!("Divisor={}", divisor);
        let new_baud = ss.baud_base / divisor;

        // Set the serial settings for DMX
        let mut new_ss = ss;
        new_ss.custom_divisor = divisor;
        new_ss.flags = (ASYNC_SPD_CUST | ASYNC_LOW_LATENCY) as c_int;
        println!("Setting DMX baud rate to: {} Hz", ss.baud_base / divisor);
        println!(
            "Speed error rate: {}%",
            new_baud as f64 / 250000f64 * 100.0f64
        );
        serial::set_serial(fd, &new_ss)?;
        Ok(ResetMode::ResetSerial((ss, oldtios)))
    }

    pub fn open(path: &str, mode: Mode) -> Result<Self, std::io::Error> {
        let fd = open(path)?;
        let reset = match mode {
            Mode::Termios2 => Self::configure_termios2(fd),
            Mode::SetSerial => Self::configure_set_serial(fd),
        }?;
        Ok(Port { fd, reset })
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, std::io::Error> {
        // Make sure everything is written to the port before sending next DMX frame
        unsafe {
            tcdrain(self.fd);
        }
        serial::set_break(self.fd)?;
        // sleep for 138 us - Break (BRK)
        spin_sleep(core::time::Duration::from_micros(138));
        serial::clear_break(self.fd)?;
        // sleep for 12 us - mark after break (MAB)
        //spin_sleep(core::time::Duration::from_micros(12));
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
