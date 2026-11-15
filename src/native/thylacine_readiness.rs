// Pouch exposes poll(2) for both socket slots and the cancellation self-pipe.
#[cfg(target_os = "thylacine")]
mod pouch_readiness {
    use super::{Descriptor, R, ReadinessBackend, SIGNAL_TOKEN};
    use std::{os::fd::RawFd, time::Duration};

    #[repr(C)]
    struct PollFd {
        fd: RawFd,
        events: i16,
        revents: i16,
    }

    unsafe extern "C" {
        fn poll(fds: *mut PollFd, count: std::ffi::c_ulong, timeout_ms: i32) -> i32;
    }

    #[derive(Default)]
    pub(super) struct PouchReadiness {
        // poll requires a contiguous pollfd array. Tokens follow the same slot order.
        fds: Vec<PollFd>,
        tokens: Vec<u64>,
    }

    impl ReadinessBackend for PouchReadiness {
        fn register(&mut self, token: u64, descriptor: Descriptor) -> R<()> {
            if descriptor.fd < 0 {
                return Err("E_HOST: invalid readiness descriptor".into());
            }
            if self.fds.iter().any(|slot| slot.fd == descriptor.fd) {
                return Err("E_HOST: descriptor is already registered for readiness".into());
            }
            const POLLIN: i16 = 1;
            const POLLOUT: i16 = 4;
            self.fds.push(PollFd {
                fd: descriptor.fd,
                events: if descriptor.writable { POLLOUT } else { POLLIN },
                revents: 0,
            });
            self.tokens.push(token);
            Ok(())
        }

        fn deregister(&mut self, fd: RawFd) -> R<()> {
            let slot = self
                .fds
                .iter()
                .position(|p| p.fd == fd)
                .ok_or("E_HOST: descriptor has no readiness registration")?;
            self.fds.swap_remove(slot);
            self.tokens.swap_remove(slot);
            Ok(())
        }

        fn wait(&mut self, timeout: Option<Duration>) -> R<Vec<u64>> {
            if self.fds.is_empty() {
                std::thread::sleep(
                    timeout.ok_or("E_HOST: indefinite readiness wait without descriptors")?,
                );
                return Ok(Vec::new());
            }
            // Pouch delivers process notes on return to userspace. A thread
            // parked indefinitely in poll cannot run the signal handler that
            // writes its self-pipe. Yield periodically only for signal-aware
            // launches. Empty results leave all adapter operations parked.
            const SIGNAL_CHECK: Duration = Duration::from_millis(50);
            let timeout = if self.tokens.contains(&SIGNAL_TOKEN) {
                Some(timeout.unwrap_or(SIGNAL_CHECK).min(SIGNAL_CHECK))
            } else {
                timeout
            };
            let timeout_ms = timeout.map_or(-1, |duration| {
                duration
                    .as_nanos()
                    .div_ceil(1_000_000)
                    .min(i32::MAX as u128) as i32
            });
            // SAFETY: pouch nfds_t is unsigned long. poll borrows this initialized
            // array for the call and writes its revents fields.
            let count = unsafe { poll(self.fds.as_mut_ptr(), self.fds.len() as _, timeout_ms) };
            if count < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    // Let the scheduler observe signals and recompute deadlines.
                    return Ok(Vec::new());
                }
                return Err(format!("E_HOST: readiness wait failed: {error}"));
            }
            const POLLNVAL: i16 = 0x20;
            let mut ready = Vec::with_capacity(count as usize);
            for (fd, token) in self.fds.iter().zip(&self.tokens) {
                if fd.revents & POLLNVAL != 0 {
                    return Err(format!("E_HOST: invalid registered descriptor {}", fd.fd));
                }
                // HUP and ERR also wake the owner to observe EOF or the I/O error.
                if fd.revents != 0 {
                    ready.push(*token);
                }
            }
            Ok(ready)
        }
    }
}

#[cfg(target_os = "thylacine")]
fn rust_readiness() -> R<Option<Box<dyn ReadinessBackend>>> {
    Ok(Some(Box::new(pouch_readiness::PouchReadiness::default())))
}
