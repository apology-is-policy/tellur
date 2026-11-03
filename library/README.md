# Tellur source library

These first reusable slices of the standard-library work are extracted from the
maintained settings service: JSON files in 0.75 and bounded loopback HTTP as a
library follow-up on compiler 0.76. They use explicit file imports and
manifest-selected Rust mechanisms. It is experimental source code, with no package
resolver, implicit prelude or stable binary ABI.

## JSON string-map files

Import [json_file.tellur](json_file.tellur) with a relative path and a local namespace.
The [standalone example](../examples/json_file/main.tellur) and
[settings module](../applications/settings/settings.tellur) use the same source.
Both provide the four required primitives: `file_read`, `file_replace`,
`json_decode` and `json_encode`.

| Control | Parameters | Outcomes |
| --- | --- | --- |
| `load` | `ref string path, u32 byte_limit` | `loaded { map<string, string> values }`, `missing`, `invalid { string message }`, `read_failed { string message }`, `too_large` |
| `replace` | `ref string path, ref map<string, string> values, u32 byte_limit` | `saved`, `encode_failed { string message }`, `write_failed { string message }`, `commit_uncertain { string message }`, `too_large` |

The outcomes are named `load_result` and `replace_result`. Inputs are shared borrows
for the call; returned values and diagnostic strings are owned. Neither contract
chooses process exit codes or defaults for missing data. The settings application
maps `missing` to an empty configuration and several failures to `storage_failed`;
another caller can preserve those distinctions.

The settings application's `remove` control combines `load`, the built-in
`map_remove` and `replace`. A missing key returns before encoding or writing, while
a present key is reported removed only after `saved`. Replacement uncertainty stays
visible. This is caller policy using the same library contracts; the library itself
does not impose settings-key validation or read/modify/write transaction semantics.

`load` reads at most `byte_limit + 1` bytes to detect overflow before decoding.
Limits count UTF-8 bytes, not characters. The reader returns `missing` on the host's
not-found error, `too_large` before attempting UTF-8 decoding when over the limit,
and `read_failed` for I/O or invalid UTF-8. Valid UTF-8 must encode exactly one JSON
object with unique keys and string values. Duplicate keys, other value types,
malformed syntax and trailing non-whitespace content produce `invalid`. Empty files
are invalid JSON; an empty JSON object is valid. Keys may contain whitespace or be
empty: application validation belongs to the caller.

`replace` serializes the complete map with deterministic ascending keys and pretty
JSON, then compares its UTF-8 length with `byte_limit`. Encoding allocates the string
before checking this file-size limit; this is not an allocation budget. An encoding
failure does not attempt a write. The host writes and synchronizes a new temporary
file in the destination directory before attempting replacement. Preparation errors
produce `write_failed`; after a replacement attempt, an error is `commit_uncertain`.
The caller must inspect/reload instead of assuming rollback. Failed temporary cleanup
is appended to the diagnostic. Temporary names begin `.tellur-file-`.

Successful replacement does not synchronize the parent directory and is not a
crash-durability guarantee. There is no cross-process lock, compare-and-swap,
permission preservation or coordination between load and replace. Forced termination
can leave temporary files. These are explicit bounds of this first storage profile.

## Shared Rust mechanisms

[host/text_file.rs](host/text_file.rs) and [host/json_string_map.rs](host/json_string_map.rs)
implement the four primitive signatures. A manifest explicitly embeds each source
as a child module of its root adapter. The root re-exports the operations required by
the generated interface. See the independent example's
[manifest](../examples/json_file/manifest.json) and [adapter](../examples/json_file/host.rs).
Its lockfile needs only the compiler-provided serde dependencies; it does not depend
on the settings service or HTTP parser.

From the repository root, with the current compiler built:

```sh
./bin/tellurc check examples/json_file/main.tellur
./bin/tellurc build examples/json_file/main.tellur --release
./examples/json_file/build/main --entry save -- /tmp/tellur-library.json theme dark
./examples/json_file/build/main -- /tmp/tellur-library.json
```

The example's `save` entry replaces the whole document with one pair; it is not a
read/modify/write operation. `main` reports a missing file with status 3, prints a
loaded document with status 0 and reports other failures with status 1. `save` uses
status 4 for uncertain replacement and status 1 for other failures.

Module paths resolve relative to the manifest. Primitive declarations are still
project-wide, with exact signatures; importing a Tellur file does not auto-load a
manifest or Rust code. [Rust adapter documentation](../docs/rust-adapters.md#explicit-source-modules)
describes composition, trust and build boundaries. The current modules are synchronous;
they introduce no Rust future or executor contract and leave Tellur's planned async
and LLVM coroutine model open.

The [library tests](../tests/json_file_library.rs) check outcome preservation and
expansion, live byte limits, UTF-8/JSON failures, replacement uncertainty and cleanup.
The existing settings CLI/server suites exercise the extracted implementation in
its original application, including fault contingency and subsequent requests.

## Bounded loopback HTTP

Import [loopback_http.tellur](loopback_http.tellur) under a local namespace. Its
seven public controls expose the transport's closed outcomes without choosing routes,
ordinary response statuses or supervision policy. The retained opaque types
`http_listener` and `http_connection` and matching `http_*` primitives must be supplied
by the project manifest. The [independent hello service](../examples/http_hello/README.md)
and [settings server](../applications/settings/server.tellur) use the same source.

| Control | Inputs | Result contract |
| --- | --- | --- |
| `listen` | `u32 port` | `listen_result`: owned listener and authority, or `failed` |
| `accept` | `ref http_listener listener, u32 idle_ms` | `accept_result`: owned connection, `idle`, or `failed` |
| `receive` | `ref http_connection connection` | `receive_result`: owned method, target, headers and UTF-8 body, or a distinct framing, limit, deadline, disconnect or I/O outcome |
| `send` | shared connection, status, shared body/media/Allow text | `send_result`: `sent` or `failed` |
| `close` | owned connection | `close_result`: `closed` or `failed`; consumes the owner either way |
| `release` | owned listener | `released`; consumes the owner |
| `reply_failure` | shared connection | `failure_reply_result`: `sent`, `unavailable` or `failed` |

`receive_result` preserves `malformed { string message }`, `headers_too_large`,
`body_too_large`, `unsupported_transfer`, `unsupported_expectation`, `timed_out`,
`disconnected` and `failed { string message }`. Other `failed` alternatives also
carry a diagnostic string. Callers must complete each acquired owner explicitly,
including after an I/O failure. Inputs borrowed for a call are not retained by its
result. Received strings and header maps are owned; they do not borrow a socket buffer.
Receive can be attempted only once per connection, including after a read failure.
A second call returns `failed` immediately without reading or changing the first
request's state. Send is likewise attempted at most once after metadata validation.

Only `close` and `reply_failure` are marked `@contingent`; they remain callable in
ordinary work as well. A contingency may use a retained connection to attempt a fixed
generic 500 and complete it before recovery. `reply_failure` returns `unavailable`
when transmission has already started, preventing a second response. A failed send
may already have written bytes, so it does not promise rollback or retryability.
The wrapper never turns a source fault into an outcome. A fault, if any, is handled
by the caller's explicit boundary and its abandoned normal continuation stays abandoned.

The implementation preserves the settings service's fixed synchronous development
profile, adding the repeated-receive guard needed by independent callers:

- Listen only on IPv4 loopback; one HTTP/1.1 request per connection. Port 0 is allowed.
- Require CRLF and one nonempty Host header; reject duplicate headers and non-origin-form
  targets. At most 64 headers and 16 KiB of header bytes.
- Accept one decimal Content-Length, defaulting to zero. Request bodies are at most
  1 MiB and must be UTF-8. Transfer encodings and Expect are unsupported; conflicting
  transfer encoding and length are malformed. Pipelined requests are never processed.
- Share a two-second deadline for request reads; allow two seconds for sending, with
  a 2 MiB response body limit. HEAD suppresses body bytes while retaining its length.
- Support statuses 200, 400, 403, 404, 405, 408, 413, 415, 417, 431, 500 and 501;
  media types are `application/json` and `text/plain; charset=utf-8`. Allow metadata
  is validated. Responses close the connection and carry no-store/nosniff headers.
- Complete connections by half-closing writes, draining at most 1 MiB + 16 KiB for
  at most 100 ms, then releasing the descriptor. This is not a delivery guarantee.

The caller chooses Host matching, browser-origin policy and route interpretation.
`accept` validates an idle timeout of 1..60,000 ms. Listen, accept, read, write and
completion operations are blocking; these deadlines do not define an async scheduler.
There is no TLS, authentication or concurrency in this profile.

[host/loopback_http.rs](host/loopback_http.rs) is embedded by explicit `rust.modules`
and depends on the project's locked `httparse` version. The root adapter re-exports
`Listener`, `Connection` and the seven primitive functions. See the standalone
[manifest](../examples/http_hello/manifest.json) and [adapter](../examples/http_hello/host.rs).
This uses the existing typed Rust source boundary and retained-resource rules;
it creates no stable Rust binary ABI, future/executor contract or coroutine restriction.

[Library tests](../tests/loopback_http_library.rs) check forwarding and cleanup,
contingency admission, independent live routes, Unicode/HEAD behavior, idle shutdown,
invalid configuration and fault recovery. The full maintained settings HTTP regression
also runs through these shared controls and transport mechanisms.


A [private owned-receive experiment](../docs/async-execution.md#owned-http-receive-and-deadline-model-e1f6a)
compares partial parsing, declared outcomes, HEAD state and fixed read deadlines
against this implementation under a deterministic transport. It is preparation for
async I/O; these public controls and Rust wrappers retain the synchronous profile
above. The follow-up [live transport experiment](../docs/async-execution.md#concrete-private-http-receivereadiness-e1f6b)
reuses that parser with private nonblocking sockets and readiness generations. It
checks the original deadline, partial input, EOF and descriptor retirement on macOS.
It supplies no public async HTTP control or new manifest execution mode.
