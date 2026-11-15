// Included in generated Rust adapters to exercise the platform's readiness handles.
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

#[cfg(unix)]
fn stream_pair() -> std::io::Result<(Stream, Stream)> {
    Stream::pair()
}

#[cfg(windows)]
type Stream = std::net::TcpStream;

#[cfg(windows)]
fn stream_pair() -> std::io::Result<(Stream, Stream)> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let writer = Stream::connect(listener.local_addr()?)?;
    let (reader, _) = listener.accept()?;
    Ok((reader, writer))
}
