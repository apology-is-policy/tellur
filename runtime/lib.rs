//! Native runtime linked into Villanelle executables through the versioned C ABI.
#![allow(dead_code)]

#[path = "../src/native/runtime.rs"]
pub mod runtime;
use runtime::{R, ReadinessBackend};

#[cfg(feature = "polled")]
use runtime::Descriptor;
#[cfg(all(feature = "polled", target_os = "thylacine"))]
use runtime::SIGNAL_TOKEN;
#[cfg(feature = "polled")]
include!("../src/native/mio_readiness.rs");
#[cfg(feature = "polled")]
include!("../src/native/windows_readiness.rs");
#[cfg(feature = "polled")]
include!("../src/native/thylacine_readiness.rs");

#[cfg(not(feature = "polled"))]
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    Ok(None)
}

#[cfg(all(windows, feature = "polled", test))]
#[test]
fn windows_socket_readiness_tracks_live_registrations() {
    use std::{
        io::Write,
        net::{TcpListener, TcpStream},
        os::windows::io::AsRawSocket,
        time::Duration,
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let mut writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (reader, _) = listener.accept().unwrap();
    let raw = reader.as_raw_socket();
    let mut backend = rust_readiness().unwrap().unwrap();
    let interest = Descriptor {
        fd: raw,
        writable: false,
    };
    backend.register(17, interest).unwrap();
    assert!(backend.register(18, interest).is_err());
    assert!(backend.wait(Some(Duration::ZERO)).unwrap().is_empty());
    writer.write_all(b"ready").unwrap();
    assert_eq!(backend.wait(Some(Duration::from_secs(2))).unwrap(), [17]);
    backend.deregister(raw).unwrap();
    assert!(backend.deregister(raw).is_err());
    backend
        .register(
            23,
            Descriptor {
                fd: raw,
                writable: true,
            },
        )
        .unwrap();
    assert_eq!(backend.wait(Some(Duration::from_secs(2))).unwrap(), [23]);
    backend.deregister(raw).unwrap();
}

#[cfg(all(target_os = "thylacine", feature = "polled", test))]
#[test]
fn pouch_poll_tracks_registrations_and_wakes_on_cancellation_pipe() {
    use std::{
        fs::File,
        io::{Read, Write},
        os::fd::{AsRawFd, FromRawFd},
        time::Duration,
    };
    fn wake_pipe() -> (File, File) {
        // Use the same pouch pipe call as the process signal handler.
        unsafe extern "C" {
            fn pipe(fds: *mut i32) -> i32;
        }
        let mut fds = [-1; 2];
        assert_eq!(unsafe { pipe(fds.as_mut_ptr()) }, 0);
        // A successful pipe call transfers ownership of both ends to this test.
        unsafe { (File::from_raw_fd(fds[0]), File::from_raw_fd(fds[1])) }
    }

    let (mut reader, mut writer) = wake_pipe();
    let (mut wake, mut signal) = wake_pipe();
    let mut backend = rust_readiness().unwrap().unwrap();
    let interest = Descriptor {
        fd: reader.as_raw_fd(),
        writable: false,
    };
    backend.register(17, interest).unwrap();
    assert!(backend.register(19, interest).is_err());
    backend
        .register(
            SIGNAL_TOKEN,
            Descriptor {
                fd: wake.as_raw_fd(),
                writable: false,
            },
        )
        .unwrap();
    assert!(
        backend
            .wait(Some(Duration::from_millis(2)))
            .unwrap()
            .is_empty()
    );

    signal.write_all(b"!").unwrap();
    assert_eq!(
        backend.wait(Some(Duration::from_secs(2))).unwrap(),
        [SIGNAL_TOKEN]
    );
    let mut byte = [0u8];
    wake.read_exact(&mut byte).unwrap();
    backend.deregister(wake.as_raw_fd()).unwrap();
    assert!(backend.deregister(wake.as_raw_fd()).is_err());
    writer.write_all(b"t").unwrap();
    assert_eq!(backend.wait(Some(Duration::from_secs(2))).unwrap(), [17]);
    reader.read_exact(&mut byte).unwrap();
    assert_eq!(byte, *b"t");

    backend.deregister(reader.as_raw_fd()).unwrap();
    backend
        .register(
            29,
            Descriptor {
                fd: writer.as_raw_fd(),
                writable: true,
            },
        )
        .unwrap();
    assert_eq!(backend.wait(Some(Duration::from_secs(2))).unwrap(), [29]);
    backend.deregister(writer.as_raw_fd()).unwrap();
    backend.register(31, interest).unwrap();
    drop(writer);
    assert_eq!(backend.wait(Some(Duration::from_secs(2))).unwrap(), [31]);
    assert_eq!(reader.read(&mut byte).unwrap(), 0);
}
