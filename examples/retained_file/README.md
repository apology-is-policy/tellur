# Retained Rust file

This small example opens a Rust `std::fs::File`, borrows it to read one chunk, and
explicitly releases it before handling the saved read result. The Tellur control
keeps release on both successful and failed reads. Removing `file_release(file)`
produces `E_RESOURCE` in the compiler and VS Code.

From the repository root, after building the compiler:

```sh
./bin/tellurc check examples/retained_file/main.tellur
./bin/tellurc build examples/retained_file/main.tellur
./examples/retained_file/build/main --json -- playground/message.txt
```

For a fresh clone built with Cargo, replace `./bin/tellurc` with
`./target/release/tellurc`. The JSON report contains the named outcome, raw bytes
and open/read/release events. `data` and `end` have status 0; `open_failed` and
`read_failed` have status 1. A read returns at most 4096 bytes and can return a
short chunk; this example does not read the entire file or decode UTF-8. Empty
files produce `end`.

`file_release` drops the read-only Rust file and reports `released`. This is
descriptor release, with Rust's unreported close-error behavior; it does not
promise durability or a checked fallible close. See the
[resource contract](../../docs/rust-adapters.md#retained-resources-049) for ownership,
retry results, host teardown and the limits of symbolic scenario handles.
