fn main() {
    let (connection, threads) = lsp_server::Connection::stdio();
    if let Err(error) = tellur::lsp::serve(connection) {
        eprintln!("tellur-lsp: {error}");
        std::process::exit(1);
    }
    if let Err(error) = threads.join() {
        eprintln!("tellur-lsp transport: {error}");
        std::process::exit(1);
    }
}
