// Private deterministic input corpus. Each Blocked event is an actual WouldBlock
// result seen by ReceiveState; no scheduler-inserted initial wait is used.
thread_local! {static READS: Cell<usize> = const {Cell::new(0)};}
pub fn read_count() -> usize {
    READS.with(Cell::get)
}
pub fn open_case(name: String) -> rust_api::open_case_outcome {
    use Input::*;
    let case = std::env::var("TELLUR_HTTP_CASE").unwrap();
    let instant = std::env::var("TELLUR_HTTP_INSTANT").unwrap() == "true";
    let label = match name.as_str() {
        "left" => case.as_str(),
        "right" => "failed",
        "other" => "request",
        _ => panic!("unknown owner"),
    };
    let waits = match name.as_str() {
        "left" => {
            if instant {
                0
            } else {
                2
            }
        }
        "right" => 4,
        "other" => 7,
        _ => unreachable!(),
    };
    let mut events = (0..waits).map(|_| event(0, Blocked)).collect::<Vec<_>>();
    let bytes = |s: &str| Bytes(s.as_bytes().to_vec());
    let input = match label {
        "request" => {
            let body = format!("{name}:🙂");
            bytes(&format!(
                "HEAD /{name} HTTP/1.1\r\nHost: local\r\nX-Name: {name}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            ))
        }
        "malformed" => bytes("GET / HTTP/1.0\r\nHost: local\r\n\r\n"),
        "headers_too_large" => bytes(&format!(
            "GET / HTTP/1.1\r\nHost: local\r\nX: {}",
            "x".repeat(HEADER_LIMIT)
        )),
        "body_too_large" => {
            bytes("GET / HTTP/1.1\r\nHost: local\r\nContent-Length: 1048577\r\n\r\n")
        }
        "unsupported_transfer" => {
            bytes("GET / HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n")
        }
        "unsupported_expectation" => {
            bytes("GET / HTTP/1.1\r\nHost: local\r\nExpect: 100-continue\r\n\r\n")
        }
        "timed_out" => Error(io::ErrorKind::TimedOut),
        "disconnected" => Eof,
        "failed" => Error(io::ErrorKind::PermissionDenied),
        _ => panic!("unknown outcome"),
    };
    // With delayed timeout, the final wait advances to the original two-second
    // deadline, before this late transport event can be consumed.
    events.push(event(
        if label == "timed_out" && !instant {
            2100
        } else {
            0
        },
        input,
    ));
    let mut connection = Connection::new(events);
    connection.name = name;
    connection
        .trace
        .borrow_mut()
        .push(format!("make {}", connection.name));
    rust_api::open_case_outcome::opened(connection)
}
pub fn checkpoint() -> rust_api::checkpoint_outcome {
    TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        for name in ["left", "right"] {
            assert!(trace.iter().any(|s| s == &format!("state drop {name}")));
            assert!(
                trace
                    .iter()
                    .any(|s| s.starts_with(&format!("resource drop {name} ")))
            );
        }
        assert!(!trace.iter().any(|s| s.starts_with("resource drop other ")));
        trace.push("checkpoint".into());
    });
    rust_api::checkpoint_outcome::done
}
