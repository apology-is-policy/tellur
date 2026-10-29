# Settings service: CLI and loopback HTTP

This is Tellur's maintained application: a local JSON settings CLI and, since
compiler 0.50, a sequential loopback HTTP service using the same domain module.

`settings.tellur` owns defaults, key validation, lookup/update decisions and failure
mapping. `host.rs` implements file, JSON and text mechanisms through typed Rust
adapters generated from `manifest.json`. The committed `Cargo.lock` makes native
builds use the declared dependency graph.

Since compiler 0.54, validation and storage failures use direct arm exits such as
`invalid => exit invalid_key`. The continuing success paths remain labeled; the
public contracts and host mechanisms are unchanged.

`http.tellur` owns endpoint policy, request validation and HTTP response choices.
`server.tellur` owns acceptance, connection budgets and explicit completion. Rust
provides sockets, bounded HTTP parsing through pinned `httparse` 1.10.1, and response
framing; it does not route requests or decide which settings to change.

From the repository root:

```sh
cargo build --locked --release --bins
./target/release/tellurc check applications/settings/main.tellur
./target/release/tellurc build applications/settings/main.tellur --release

./applications/settings/build/main --entry set -- /tmp/tellur-settings.json theme dark
./applications/settings/build/main --entry get -- /tmp/tellur-settings.json theme
./applications/settings/build/main -- /tmp/tellur-settings.json
```

The last command prints the complete configuration. A missing file acts as an empty
map; the first successful update creates it. To try the supplied sample, copy
`sample.json` to your chosen data path. A key must be nonempty and have no surrounding
Unicode whitespace. Values are strings. JSON must contain an object with unique
keys and string values; malformed JSON, duplicate keys and non-string values are
rejected before a write. Reads and serialized updates have a 1 MiB limit.

An update writes a new temporary file in the destination directory, synchronizes
its contents, then attempts replacement. Preparation failures leave the existing
destination alone. A replacement error produces `commit_uncertain`: callers must
inspect/reload rather than assume rollback. Failed temporary cleanup is included
in the error detail. Successful replacement is **not a crash-durability guarantee**:
the directory entry is not synchronized. This is a single-writer slice, with no
cross-process locking, compare-and-swap, permission preservation, or transactional
coordination between reads and writes. A forced process exit may leave a temporary
file. These limitations are application work, not promises hidden in `saved`.

The launcher prints the final outcome and returns its configured OS status:

| Completion | Status |
| --- | --- |
| `done`, `saved` | 0 |
| `failed`, `invalid_file`, `storage_failed`, `too_large` | 1 |
| `invalid_key` | 2 |
| `missing` | 3 |
| `commit_uncertain` | 4 |

`main` and `get` use annotated entry contracts in `main.tellur`. `set` uses a
manifest entry mapping, keeping the reusable domain contract free of terminal
policy. `--json` preserves the complete outcome/payload; scripts must still inspect
it when distinguishing failure details. Execution faults report stderr and status 1.
See [process entry semantics](../../docs/reference/process-entry.md).

In VS Code, open `main.tellur` and use **Tellur: Build and Run**. Set the entry and
arguments in workspace settings, for example:

```json
{
  "tellurLanguage.entryRegion": "set",
  "tellurLanguage.programArguments": ["/tmp/tellur-settings.json", "theme", "dark"]
}
```

## Run the HTTP service

After building `main.tellur` as above, run:

```sh
./applications/settings/build/main --entry serve -- /tmp/tellur-settings.json 8080 100 60000
```

Arguments are **file path, port, maximum accepted connections, idle milliseconds**.
The server prints its URL immediately. Port 0 asks the OS for an available port;
the printed URL contains the actual port. The address is always `127.0.0.1`.
The connection budget must be 1..1,000,000, and idle timeout 1..60,000 ms. Invalid
configuration exits with status 2 without opening a socket. An occupied port is a
declared `failed` completion with status 1.

In another terminal, while the service is running:

```sh
curl -i http://127.0.0.1:8080/settings

curl -i -X PUT http://127.0.0.1:8080/settings \
  -H 'Content-Type: application/json' \
  --data '{"key":"theme","value":"dark"}'

curl -i -X POST http://127.0.0.1:8080/lookup \
  -H 'Content-Type: application/json' \
  --data '{"key":"theme"}'
```

| Endpoint | Body | Successful response |
| --- | --- | --- |
| `GET /settings` | Empty | 200, the complete JSON object |
| `PUT /settings` | Exactly `key` and `value`, both strings | 200, `{"saved":true}` |
| `POST /lookup` | Exactly one string field, `key` | 200, `{"value":"…"}` |

Missing keys and unknown endpoints return 404. Invalid JSON, duplicate object
keys, wrong fields and invalid setting keys return 400. Writes/lookups require
`application/json` or `application/json; charset=utf-8`; other media types return
415. Other methods return 405 with `Allow`. HEAD receives headers without a body,
including its 405 response. Targets are exact: queries and alternate encoded paths
do not select an endpoint.

Storage failures return 500; `commit_uncertain` is explicitly identified in the
response, telling the caller to reload before retrying. The existing file-size
limits and persistence qualifications above still apply. Failed response delivery
does not roll back a successful update or establish whether the client received it.

This development service has no authentication or TLS and only binds loopback.
Host must match the literal printed `127.0.0.1:port` authority. Requests carrying
Origin are rejected with 403; no CORS access is granted. The intended client is a
local CLI, not a browser UI or a reverse proxy.

## HTTP profile and completion

One connection carries one HTTP/1.1 request and response, then closes. The parser
requires CRLF, one Host field, at most 64 headers and 16 KiB of header bytes.
Duplicate headers are rejected. Bodies use a single decimal Content-Length and
are limited to 1 MiB of UTF-8. An absent length means an empty body. Conflicting
Transfer-Encoding/Content-Length returns 400; transfer encodings return 501 and
Expect returns 417. This is a deliberately limited HTTP/1.1 development profile,
not a claim to implement the full protocol. See
[`httparse`](https://docs.rs/httparse/1.10.1/httparse/struct.Request.html) and
[HTTP/1.1 framing](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3).

Header/body reads share a two-second deadline; response writes have a separate
two-second deadline and 2 MiB size limit. Oversized headers return 431, oversized
bodies 413, and expired request reads 408 when a reply can still be sent. Queued
pipelined requests are never executed. These bounds do not bound filesystem I/O
latency; the service is synchronous and handles one client at a time.

The connection completion operation half-closes the write side, drains at most
1 MiB + 16 KiB for at most 100 ms, and releases the descriptor. Bounded staged
closure avoids truncating ordinary rejection responses when input remains unread;
it cannot guarantee delivery under arbitrary disconnects or flooding. This follows
the teardown concern described in [RFC 9112 §9.6](https://www.rfc-editor.org/rfc/rfc9112.html#section-9.6).

Tellur always attempts connection completion after an exchange, even if reading or
writing failed. If both exchange and completion fail, both details are retained and
reported. Listener release runs after the accept loop completes or fails. Descriptor
release does not claim an observable OS close-error result or write durability.

The loop stops after its connection budget or after an idle accept timeout. Every
accepted connection counts, including malformed and disconnected clients. Normal
completion is `completed` or `idle` (status 0). Read/write/cleanup failures are
reported while the loop continues; eventual `completed_with_failures` or
`idle_with_failures` has status 1. Successfully delivered HTTP 4xx/5xx responses do
not by themselves make the server process fail. Accept failures end the loop as
`failed` (status 1). A clean peer disconnect is an ordinary connection completion.

For VS Code, select entry `serve` and arguments such as
`["/tmp/tellur-settings.json", "8080", "100", "60000"]`. Build and Run currently
captures output until the process completes, so use a known port for clients, or
build and launch from the integrated terminal to see a port-0 URL immediately.
Use small budgets for `--json`: it retains request/response traces, which can contain
settings values and grow with the number of connections.

Budget/idle completion is the graceful stop mechanism in this checkpoint. Ctrl-C
is process termination, not a Tellur cancellation protocol. Async tasks, concurrent
requests, signal-driven graceful shutdown and cancellation remain roadmap work;
no Rust future or executor ABI has been introduced.

Changing dependencies is explicit:

```sh
./target/release/tellurc lock-native applications/settings/main.tellur
```

[Adapter documentation](../../docs/rust-adapters.md) describes the generated Rust
interface and current type boundary. [Application tests](../../tests/settings_application.rs)
exercise live persistence/reload, invalid requests/files, size limits, failed writes,
uncertain replacement and cleanup details. Scripted failures agree with interpreted
and expanded execution, including native O0/O2 builds.
The [HTTP tests](../../tests/settings_http.rs) add real socket exchanges, framing
and deadline limits, occupied ports, persistence across restarts, clean disconnects,
graceful listener release and scripted read/write/cleanup failures.

## Review the library's public contract

`settings.contract.json` records the checked public interface of `settings.tellur`.
The regression suite checks this baseline against the current source. From the
compiler checkout:

```sh
./bin/tellurc contract-snapshot applications/settings/settings.tellur > /tmp/settings.contract.json
./bin/tellurc contract-diff applications/settings/settings.contract.json /tmp/settings.contract.json --check
```

Review changes before replacing the committed baseline. An added failure outcome,
changed payload or borrowing contract appears in the report. Changing an internal
limit or mapping a new internal failure into an existing public outcome may leave
the contract unchanged; this is not a behavioral-equivalence check.
The [snapshot reference](../../docs/reference/contract-snapshots.md) explains the
format and VS Code's **Tellur: Show Public Contract Snapshot** command.
