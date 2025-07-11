use clap::Parser;
use nix::poll::PollTimeout;
use nix::sys::epoll::{Epoll, EpollEvent};
use nix::sys::epoll::{EpollCreateFlags, EpollFlags};
use nix::sys::signal::{SigSet, SigmaskHow, Signal, sigprocmask};
use nix::sys::signalfd::SignalFd;
use nix::sys::time::{TimeSpec, TimeValLike};
use nix::sys::timer::{Expiration, TimerSetTimeFlags};
use nix::sys::timerfd::{ClockId, TimerFd, TimerFlags};
use num_derive::FromPrimitive;
use socket2::{Domain, Socket, Type};
use std::io::Read;
use std::net::SocketAddr;
use std::time::Duration;

mod dmx;
mod serial;

#[derive(Parser, Debug)]
#[command(name = "dmx-udp", version, about = "DMX over UDP", long_about = None)]
struct Args {
    // Path to the DMX serial device
    #[arg(long, short, default_value = "/dev/ttyUSB0")]
    device: String,

    // IP Address to bind the UDP socket
    #[arg(long, short, default_value = "0.0.0.0:1337")]
    bind: SocketAddr,

    // Mode of configuration
    #[arg(value_enum, long, short, default_value = "termios2")]
    mode: dmx::Mode,

    // Throttle DMX writes to avoid flooding
    #[arg(long, short, default_value = "45")]
    throttle: u64,

    #[arg(long, short)]
    wait_udp: bool,

    #[arg(long, short)]
    debug: bool,
}

fn get_domain(socket_addr: SocketAddr) -> Domain {
    match socket_addr {
        SocketAddr::V4(_) => Domain::IPV4,
        SocketAddr::V6(_) => Domain::IPV6,
    }
}

#[derive(FromPrimitive)]
#[repr(u64)]
enum Event {
    Signal = 1,
    UDP = 2,
    DMX = 3,
    Tick = 4,
}

fn main() -> std::io::Result<()> {
    let mut exiting = false; // Flag to indicate if the program is exiting
    let mut dmx_data = [0u8; 513]; // DMX data buffer

    let args = Args::parse();
    let mut socket = Socket::new(get_domain(args.bind), Type::DGRAM, None)?;
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    println!("Binding to: {}", args.bind);
    socket.bind(&args.bind.into())?;

    println!(
        "Opening DMX device: {} in {:?} mode",
        args.device, args.mode
    );
    let dmx_port = dmx::Port::open(args.device.as_str(), args.mode)?;
    let mask = SigSet::from_iter([Signal::SIGINT, Signal::SIGTERM, Signal::SIGUSR1]);
    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;

    let sigfd = SignalFd::new(&mask)?;
    let ticker = TimerFd::new(ClockId::CLOCK_MONOTONIC, TimerFlags::empty())?;
    let expiration = Expiration::IntervalDelayed(TimeSpec::seconds(1), TimeSpec::seconds(1));
    ticker.set(expiration, TimerSetTimeFlags::empty())?;

    let epoll = Epoll::new(EpollCreateFlags::empty())?;
    epoll.add(
        &sigfd,
        EpollEvent::new(EpollFlags::EPOLLIN, Event::Signal as u64),
    )?;
    epoll.add(
        &socket,
        EpollEvent::new(EpollFlags::EPOLLIN, Event::UDP as u64),
    )?;
    epoll.add(
        &dmx_port,
        EpollEvent::new(EpollFlags::EPOLLOUT, Event::DMX as u64),
    )?;
    epoll.add(
        &ticker,
        EpollEvent::new(EpollFlags::EPOLLIN, Event::Tick as u64),
    )?;

    let mut event_buffer = [EpollEvent::empty(); 10];

    let mut dmx_frames = 0; // Counter for DMX frames processed
    let mut udp_frames = 0; // Counter for UDP frames received
    let mut last_dmx_frames = 0; // Last count of DMX frames processed
    let mut last_udp_frames = 0; // Last count of UDP frames received
    let mut last_udp_delta = 0;
    let mut last_dmx_delta = 0;
    let mut first_udp_packet = false;

    let mut dmx_write_throttle = TimeSpec::zero();

    while !exiting {
        let n = epoll.wait(&mut event_buffer, PollTimeout::NONE)?;
        for event in event_buffer[..n].iter().as_slice() {
            if exiting {
                break;
            }
            let event = match num::FromPrimitive::from_u64(event.data()) {
                Some(val) => val,
                None => {
                    println!("Received unknown event number: {:?}", event.data());
                    continue;
                }
            };
            match event {
                Event::Signal => {
                    if let Some(info) = sigfd.read_signal()? {
                        match (info.ssi_signo as i32).try_into() {
                            Ok(Signal::SIGINT) => {
                                // Handle SIGINT signal
                                println!("Received SIGINT signal, exiting...");
                                exiting = true;
                            }
                            Ok(Signal::SIGTERM) => {
                                // Handle SIGTERM signal
                                println!("Received SIGTERM signal, exiting...");
                                exiting = true;
                            }
                            Ok(Signal::SIGUSR1) => {
                                println!("FPS[UDP:{:?} DMX:{:?}]", last_udp_delta, last_dmx_delta);
                            }
                            _ => {
                                // Handle other signals
                                println!("Received signal: {:?}", info.ssi_signo);
                            }
                        }
                    }
                }
                Event::UDP => {
                    // This is a non-blocking socket
                    // We need to drain all buffered frames to catch up
                    while let Ok(_size) = socket.read(&mut dmx_data[1..]) {
                        udp_frames += 1; // Increment UDP frame count
                        first_udp_packet = true;
                    }
                }
                Event::DMX => {
                    if !first_udp_packet && args.wait_udp {
                        continue;
                    }
                    let now = nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC)?;
                    if now - dmx_write_throttle
                        < TimeSpec::from(Duration::from_millis(1000 / args.throttle))
                    {
                        // Throttle DMX writes to avoid flooding
                        continue;
                    }
                    // Write DMX data to the port
                    if let Err(e) = dmx_port.write(&dmx_data) {
                        eprintln!("Failed to write DMX data: {}", e);
                        exiting = true;
                    }
                    dmx_frames += 1; // Increment DMX frame count
                    dmx_write_throttle =
                        nix::time::clock_gettime(nix::time::ClockId::CLOCK_MONOTONIC)?;
                }
                Event::Tick => {
                    // Handle timer tick event
                    let _ = ticker.wait();
                    let udp_frames_delta = udp_frames - last_udp_frames;
                    let dmx_frames_delta = dmx_frames - last_dmx_frames;
                    last_udp_delta = udp_frames_delta;
                    last_dmx_delta = dmx_frames_delta;
                    if args.debug {
                        println!(
                            "UDP frames: {}, DMX frames: {}",
                            udp_frames_delta, dmx_frames_delta
                        );
                    }
                    last_udp_frames = udp_frames;
                    last_dmx_frames = dmx_frames;
                }
            }
        }
    }
    Ok(())
}
