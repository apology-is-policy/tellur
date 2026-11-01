# Tellur source library

This is the first reusable slice of the standard-library work, extracted from the
maintained settings service in compiler 0.75. It uses explicit file imports and
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
