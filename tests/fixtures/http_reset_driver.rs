// Unix-only peer setup. The parser and receiver keep their safe socket owners.
fn abort_peer(peer: TcpStream) {
    use std::os::fd::AsRawFd;
    let linger = libc::linger {
        l_onoff: 1,
        l_linger: 0,
    };
    // SAFETY: peer owns this live descriptor for the entire call. The pointer
    // and length describe an initialized linger value; setsockopt copies it.
    let result = unsafe {
        libc::setsockopt(
            peer.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_LINGER,
            std::ptr::from_ref(&linger).cast(),
            std::mem::size_of_val(&linger) as libc::socklen_t,
        )
    };
    assert_eq!(
        result,
        0,
        "abortive close setup: {}",
        io::Error::last_os_error()
    );
    drop(peer);
}

fn reset_after(bytes: &[u8]) {
    let b = Backend::new();
    let (c, mut peer, trace) = pair(&b);
    let mut s = Session::new(c);
    let original = s.pump().unwrap();
    if !bytes.is_empty() {
        assert_eq!(s.send(&mut peer, bytes), Some(original));
    }
    assert_eq!(s.connection.stream.0.bytes.get(), bytes.len());
    assert_eq!(s.connection.stream.0.resets.get(), 0);
    let head = s.connection.head.get();
    assert_eq!(head, !bytes.is_empty());
    s.duplicate();
    abort_peer(peer);
    let mut saved = vec![];
    for _ in 0..32 {
        let notice = s.notice();
        let before = s.connection.stream.0.reads.get();
        s.accept(&notice);
        assert_eq!(s.connection.stream.0.reads.get(), before);
        saved.push(notice);
        match s.pump() {
            Some(until) => assert_eq!(until, original),
            None => break,
        }
    }
    assert!(
        s.result.is_some(),
        "reset did not complete within polling bound"
    );
    // Require a real read error. EOF/read-closed readiness alone is insufficient.
    assert_eq!(s.connection.stream.0.resets.get(), 1);
    assert!(matches!(s.take(), ReceiveOutcome::disconnected));
    assert_eq!(s.deadline, Some(original));
    assert_eq!(s.connection.stream.0.bytes.get(), bytes.len());
    assert_eq!(s.connection.head.get(), head);
    s.duplicate();
    for notice in saved {
        assert!(!b.claim(&notice));
    }
    drop(s);
    retired(&trace, false);
    b.finished();
}

pub fn run() {
    reset_after(b"");
    reset_after(b"HEAD /live HTTP/1.1\r\nHost: lo");
    let mut body = HEAD.to_vec();
    body.extend_from_slice(b"ab");
    reset_after(&body);
    println!(
        "live HTTP reset cases: 3; actual ConnectionReset, fixed deadline and retirement verified"
    );
}
