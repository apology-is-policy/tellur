//! Native runtime linked into Tellur executables through the versioned C ABI.
#![allow(dead_code)]

#[path = "../src/native/runtime.rs"]
pub mod runtime;
use runtime::{R, ReadinessBackend};

#[cfg(feature = "polled")]
use runtime::Descriptor;
#[cfg(feature = "polled")]
include!("../src/native/mio_readiness.rs");
#[cfg(feature = "polled")]
include!("../src/native/windows_readiness.rs");

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
