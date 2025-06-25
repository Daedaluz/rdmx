#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
use libc::*;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

pub fn tcgets2(fd: c_int) -> Result<termios2, std::io::Error> {
    let mut tios = unsafe { std::mem::zeroed::<termios2>() };
    let res = unsafe { ioctl(fd, TCGETS2 as _, &mut tios) };
    if res == 0 {
        Ok(tios)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn tcsets2(fd: i32, tios: &termios2) -> Result<(), std::io::Error> {
    let res = unsafe { ioctl(fd, TCSETS2 as _, tios) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn set_brk(fd: c_int) -> Result<(), std::io::Error> {
    let res = unsafe { ioctl(fd, TIOCSBRK as _) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn clear_break(fd: c_int) -> Result<(), std::io::Error> {
    let res = unsafe { ioctl(fd, TIOCCBRK as _) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn get_serial(fd: c_int) -> Result<serial_struct, std::io::Error> {
    let mut ss = unsafe { std::mem::zeroed::<serial_struct>() };
    let res = unsafe { ioctl(fd, TIOCGSERIAL as _, &mut ss) };
    if res == 0 {
        Ok(ss)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn set_serial(fd: c_int, ss: &serial_struct) -> Result<(), std::io::Error> {
    let res = unsafe { ioctl(fd, TIOCSSERIAL as _, ss) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn tcgets(fd: c_int) -> Result<termios, std::io::Error> {
    let mut termios = unsafe { std::mem::zeroed::<termios>() };
    let res = unsafe { tcgetattr(fd, &mut termios) };
    if res == 0 {
        Ok(termios)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub fn tcsets(fd: c_int, termios: &termios) -> Result<(), std::io::Error> {
    let res = unsafe { tcsetattr(fd, TCSANOW, termios) };
    if res == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
