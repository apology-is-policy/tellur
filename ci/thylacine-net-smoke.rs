//! Compile with the patched Thylacine Rust SDK and run the resulting binary in a private guest.
//! These checks exercise netd's native TCP files and the settings listener contract.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn poll(fds: *mut PollFd, count: std::ffi::c_ulong, timeout_ms: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

fn assert_cloexec(file: &File) {
    // F_GETFD takes no third argument on pouch and FD_CLOEXEC is bit zero.
    let flags = unsafe { fcntl(file.as_raw_fd(), 1) };
    assert!(flags >= 0, "F_GETFD: {}", io::Error::last_os_error());
    assert_ne!(flags & 1, 0, "descriptor would survive exec");
}

fn open(path: &str, writable: bool) -> io::Result<File> {
    let file = OpenOptions::new().read(true).write(writable).open(path)?;
    assert_cloexec(&file);
    Ok(file)
}

struct Connection {
    ctl: File,
    number: u32,
}

impl Connection {
    fn from_ctl(mut ctl: File) -> io::Result<Self> {
        let mut buf = [0; 16];
        let n = ctl.read(&mut buf)?;
        let number = std::str::from_utf8(&buf[..n])
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .ok_or_else(|| io::Error::other("invalid netd connection number"))?;
        Ok(Self { ctl, number })
    }

    fn new() -> io::Result<Self> {
        Self::from_ctl(open("/net/tcp/clone", true)?)
    }

    fn file(&self, name: &str, writable: bool) -> io::Result<File> {
        open(&format!("/net/tcp/{}/{name}", self.number), writable)
    }

    fn command(&mut self, command: &str) -> io::Result<()> {
        if self.ctl.write(command.as_bytes())? != command.len() {
            return Err(io::Error::other("short netd control write"));
        }
        Ok(())
    }
}

fn ready(file: &File, events: i16, ms: i32) -> io::Result<i16> {
    let mut entry = PollFd {
        fd: file.as_raw_fd(),
        events,
        revents: 0,
    };
    // poll borrows one initialized pollfd until it returns.
    if unsafe { poll(&mut entry, 1, ms) } < 0 {
        return Err(io::Error::last_os_error());
    }
    assert_eq!(entry.revents & 0x20, 0, "invalid ready descriptor");
    Ok(entry.revents)
}

fn loopback_transfers_bytes_and_half_closes_with_owned_descriptors() -> io::Result<()> {
    let mut listener = Connection::new()?;
    listener.command("announce 127.0.0.1!38172")?;
    let incoming = listener.file("ready", false)?;
    assert_eq!(ready(&incoming, 1, 15)?, 0);

    let mut client = Connection::new()?;
    client.command("connect 127.0.0.1!38172")?;
    let mut client_data = client.file("data", true)?;
    assert_ne!(ready(&incoming, 1, 2000)? & 1, 0);
    let mut peer = Connection::from_ctl(listener.file("listen", true)?)?;
    let mut data = peer.file("data", true)?;
    let watch = peer.file("ready", false)?;
    let mut reader = data.try_clone()?;
    assert_cloexec(&reader);
    assert_eq!(ready(&watch, 1, 15)?, 0);
    client_data.write_all(b"Q")?;
    assert_ne!(ready(&watch, 1, 2000)? & 1, 0);
    let mut byte = [0];
    reader.read_exact(&mut byte)?;
    assert_eq!(&byte, b"Q");
    assert_ne!(ready(&watch, 4, 2000)? & 4, 0);
    data.write_all(b"R")?;
    peer.command("hangup")?;
    client_data.read_exact(&mut byte)?;
    assert_eq!(&byte, b"R");
    assert_eq!(client_data.read(&mut byte)?, 0);
    Ok(())
}

fn occupied_loopback_port_is_exclusive() -> io::Result<()> {
    let mut owner = Connection::new()?;
    owner.command("announce 127.0.0.1!38173")?;
    let mut other = Connection::new()?;
    if other.command("announce 127.0.0.1!38173").is_ok() {
        return Err(io::Error::other("second listener claimed an occupied port"));
    }
    Ok(())
}

fn port_zero_allocates_a_concrete_loopback_endpoint() -> io::Result<()> {
    let mut listener = Connection::new()?;
    listener.command("announce 127.0.0.1!0")?;
    let mut address = String::new();
    listener
        .file("local", false)?
        .read_to_string(&mut address)?;
    let port = address
        .trim()
        .strip_prefix("127.0.0.1!")
        .and_then(|p| p.parse::<u16>().ok());
    assert!(
        port.is_some_and(|p| p != 0),
        "unexpected local endpoint {address:?}"
    );
    Ok(())
}

fn main() -> io::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("transfer") => loopback_transfers_bytes_and_half_closes_with_owned_descriptors(),
        Some("occupied") => occupied_loopback_port_is_exclusive(),
        Some("automatic") => port_zero_allocates_a_concrete_loopback_endpoint(),
        _ => Err(io::Error::other("expected transfer, occupied or automatic")),
    }
}
