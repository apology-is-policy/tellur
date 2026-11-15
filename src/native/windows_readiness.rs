// Winsock borrows each registered socket until the host deregisters it.
#[cfg(windows)]
mod windows_readiness {
    use super::{Descriptor, R, ReadinessBackend};
    use std::{collections::BTreeMap, os::windows::io::RawSocket, time::Duration};

    #[repr(C)]
    struct PollFd {
        fd: RawSocket,
        events: i16,
        revents: i16,
    }

    #[link(name = "ws2_32")]
    unsafe extern "system" {
        fn WSAPoll(fds: *mut PollFd, count: u32, timeout_ms: i32) -> i32;
        fn WSAGetLastError() -> i32;
    }

    #[derive(Default)]
    pub(super) struct SocketReadiness {
        sockets: BTreeMap<RawSocket, (u64, i16)>,
    }

    impl ReadinessBackend for SocketReadiness {
        fn register(&mut self, token: u64, descriptor: Descriptor) -> R<()> {
            use std::collections::btree_map::Entry;
            let Entry::Vacant(slot) = self.sockets.entry(descriptor.fd) else {
                return Err("E_HOST: socket is already registered for readiness".into());
            };
            const POLLRDNORM: i16 = 0x0100;
            const POLLWRNORM: i16 = 0x0010;
            let interest = if descriptor.writable {
                POLLWRNORM
            } else {
                POLLRDNORM
            };
            slot.insert((token, interest));
            Ok(())
        }
        fn deregister(&mut self, fd: RawSocket) -> R<()> {
            self.sockets
                .remove(&fd)
                .ok_or("E_HOST: socket has no readiness registration")?;
            Ok(())
        }
        fn wait(&mut self, timeout: Option<Duration>) -> R<Vec<u64>> {
            if self.sockets.is_empty() {
                std::thread::sleep(
                    timeout.ok_or("E_HOST: indefinite readiness wait without sockets")?,
                );
                return Ok(Vec::new());
            }
            let mut fds: Vec<_> = self
                .sockets
                .iter()
                .map(|(&fd, &(_, events))| PollFd {
                    fd,
                    events,
                    revents: 0,
                })
                .collect();
            let timeout_ms = timeout.map_or(-1, |duration| {
                duration
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .min(i32::MAX as u128) as i32
            });
            let count =
                u32::try_from(fds.len()).map_err(|_| "E_HOST: too many registered sockets")?;
            if unsafe { WSAPoll(fds.as_mut_ptr(), count, timeout_ms) } == -1 {
                let error = unsafe { WSAGetLastError() };
                // The scheduler recalculates the remaining deadline after interruption.
                if error == 10004 {
                    return Ok(Vec::new());
                }
                return Err(format!(
                    "E_HOST: socket readiness wait failed: {}",
                    std::io::Error::from_raw_os_error(error)
                ));
            }
            // Hangup and error flags also wake the operation so its poll observes the result.
            Ok(fds
                .iter()
                .filter(|fd| fd.revents != 0)
                .map(|fd| self.sockets[&fd.fd].0)
                .collect())
        }
    }
}

#[cfg(windows)]
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    Ok(Some(
        Box::new(windows_readiness::SocketReadiness::default()),
    ))
}
