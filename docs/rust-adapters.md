# Rust source adapters

Since compiler 0.45, project-supplied Rust wrappers can be compiled with a generated native
program. The Tellur primitive manifest is the source of argument types, closed
outcomes, ownership modes and declared effects. The compiler generates concrete
Rust result enums, conversions and exact wrapper-signature checks from it.

This is source interoperability through Cargo, not a stable Rust binary ABI. LLVM
still compiles the generated program. A future direct LLVM backend can replace this
bridge with a versioned native adapter ABI; Tellur tasks, suspension and cancellation
are not defined in terms of Rust futures or an executor.

## Declare an adapter

A complete minimal manifest:

```json
{
  "version": "0.1",
  "types": [],
  "rust": {
    "source": "host.rs",
    "lockfile": "Cargo.lock",
    "dependencies": {
      "memchr": { "version": "=2.8.3", "default_features": false }
    }
  },
  "primitives": [{
    "name": "contains_zero",
    "parameters": [{ "name": "data", "type": "bytes", "mode": "borrow" }],
    "outcomes": [{ "name": "present", "fields": [] }, { "name": "absent", "fields": [] }],
    "effects": [],
    "returns": true,
    "rust": { "execution": "immediate" }
  }]
}
```

`host.rs` supplies the identically named function:

```rust
use crate::rust_api::contains_zero_outcome;

pub fn contains_zero(data: &[u8]) -> contains_zero_outcome {
    if memchr::memchr(0, data).is_some() {
        contains_zero_outcome::present
    } else {
        contains_zero_outcome::absent
    }
}
```

For an operation `read_text`, the generated result type is
`rust_api::read_text_outcome`. Its variants have the exact Tellur outcome names.
A payload-bearing alternative is a tuple variant, with fields in manifest order:
`read_text_outcome::read(String)`, for example. Binding names are documentation;
the function and tuple types are checked by Rust during a native build. Rust code
must implement all Rust-bound primitives in the manifest, including unused ones.

Each binding must declare `execution` as `immediate` or `blocking`. This records a
host execution contract; neither introduces suspension. `immediate` is not a purity
proof. Foreign wrappers cannot opt into Tellur pure-expression evaluation. Effects
and execution classification remain promises made by the adapter author.

## Data and ownership

| Tellur manifest type | Owned Rust type | Borrowed Rust parameter |
| --- | --- | --- |
| `bool` | `bool` | `&bool` |
| `u32` | `u32` | `&u32` |
| `unit` | `()` | `&()` |
| `string` | `String` | `&str` |
| `bytes` | `Vec<u8>` | `&[u8]` |
| `vector<t>` | `Vec<T>` | `&[T]` |
| `map<k, v>` | `BTreeMap<K, V>` | `&BTreeMap<K, V>` |

Vectors and map values may nest supported types. Map keys are `string`, `u32` or
`bool`; ordering matches Tellur's map representation. The manifest `borrow` mode
means a call-duration shared borrow. `value` and `consume` deliver owned Rust
arguments, under the existing Tellur Copy/move rules. Outcomes contain owned data.
Returned references, exclusive foreign mutation, nominal
records/sums, callbacks and asynchronous adapters are not supported by this profile.
Compiler 0.49 additionally supports directly bound opaque retained resources below.

The current bridge converts runtime values and may copy borrowed input data. It
does not promise zero-copy FFI. The generated function types prevent ordinary safe
wrappers from returning borrowed input as an owning result. Arbitrary Rust remains
a trust boundary: types do not prove effect declarations, error classification,
blocking behavior or domain laws. An adapter panic terminates the native execution;
it is not converted into a recoverable Tellur alternative. No unwinding/abort or
cancellation cleanup guarantee is implied.

## Retained resources (0.49)

An opaque must-complete Tellur type can represent an owned Rust object that survives
between calls. Add a `resources` map to the top-level `rust` configuration:

```json
"types": [{"name": "input_file", "kind": "opaque", "linear": true}],
"rust": {
  "source": "host.rs",
  "lockfile": "Cargo.lock",
  "resources": {"input_file": "InputFile"}
}
```

This fragment binds `input_file` to `rust_host::InputFile`. `host.rs` must define or
publicly re-export that type, for example `pub use std::fs::File as InputFile;`.
The right-hand name is one Rust-compatible identifier, not arbitrary Rust syntax
or a qualified path. A public alias/re-export can name a dependency type. The type
must be `'static` (it cannot retain a call's borrowed arguments); it need not be
`Copy`, `Clone`, `Send` or `Sync`. Two Tellur resource names remain distinct even if
they map to the same Rust type. Duplicate resource bindings are rejected.

| Boundary use of `input_file` | Rust representation | Ownership effect |
| --- | --- | --- |
| Outcome payload | `InputFile` | Store the returned object and issue a fresh Tellur handle |
| Parameter, `mode: borrow` | `&InputFile` | Borrow the stored object for this call |
| Parameter, `mode: consume` | `InputFile` | Remove the object and transfer it into the wrapper |
| Parameter, `mode: value` | Rejected | Must-complete owners require borrow or consume |

For example, the three generated interfaces in the
[retained-file example](../examples/retained_file/host.rs) are:

```rust
pub fn file_open(path: &str) -> rust_api::file_open_outcome;
pub fn file_chunk(file: &InputFile) -> rust_api::file_chunk_outcome;
pub fn file_release(file: InputFile) -> rust_api::file_release_outcome;
```

Resources currently cross the Rust boundary directly, as parameters or individual
outcome fields. A Rust-bound `vector<input_file>`, nominal record/sum, returned
reference or exclusive borrow is not supported. Inside Tellur, the existing
resource-containing structs, sums, outcomes and collections still work; unpack an
owner before passing it to Rust. A wrapper may own a richer internal Rust object
behind a single opaque type.

The existing checker enforces consumption or transfer on every selected completion.
It rejects implicit discard, `drop`, `clone`, use after consumption and borrowing
an owner during its consumption. Multiple shared inputs may name the same object.
The bridge does not clone these objects to satisfy borrows. A shared resource
borrow protects the owner from replacement/consumption; it does not promise that
external state is immutable. Reads can advance a file offset and a listener can
accept connections. Such effects belong in the primitive's declared effects.

### Completion, failures and identity

Consumption happens regardless of which declared outcome the wrapper returns.
A failed close therefore cannot silently restore the old Tellur owner. If retry
requires retaining ownership, declare a resource payload on that outcome and
return the object explicitly. The returned handle is fresh, even when the same
Rust object is returned. There is no built-in interpretation of an operation's
name as `close`, `release` or `retry`.

Live handles are checked against invocation-owned storage before dispatch.
Unknown, consumed, wrong-type and cross-host identities are rejected; consuming
aliases are rejected before objects are removed or the wrapper is called. Trace
copies of a handle do not clone its Rust owner. Tokens are process-local diagnostic
identities, not serialized objects or cryptographic capabilities. A JSON token
from an earlier run cannot reopen a resource. Live executable entries reject
resource arguments supplied as JSON, including nested ones, before any wrapper
runs; acquire owners inside the entry instead. Scenario runs use symbolic opaque
values and bypass both the store and live wrappers.

Normal host teardown and ordinary returned runtime errors drop any retained Rust
objects. This fallback does not invoke Tellur completion controls or synthesize
their outcomes. An executable entry that returns a resource transfers it to the
host: its JSON payload records an identity that is released at teardown, not a
resource transferable to another process. Complete any required fallible protocol
before the executable entry returns. No destructor ordering, panic/abort, forced
termination or future cancellation guarantee is added by this checkpoint.

The example's `file_release` deliberately means descriptor release. Rust's
[`File` destructor](https://doc.rust-lang.org/std/fs/struct.File.html) does not report
close errors, so this operation does not claim fallible-close success or write
durability. Adapters requiring fallible completion must expose explicit outcomes.

Retained storage currently belongs to one synchronous invocation, with no global
object registry, thread-safety requirement, Rust future or executor contract. Future
Tellur tasks will need explicit resource/frame ownership and cancellation rules;
this facility does not settle them or constrain LLVM coroutine lowering.

## Dependency and build workflow

`source` and `lockfile` resolve relative to `manifest.json`, regardless of the shell
working directory. The CLI and LSP use this rule. Embedders use
`Manifest::from_json_at(text, manifest_path)`; plain `from_json` leaves relative paths
unresolved and native generation rejects them. Checking Tellur source does not read,
compile or execute Rust wrappers. Native generation reads and embeds `host.rs` into
the generated crate; wrapper-local `mod`/`include!` paths therefore refer to that
crate, not the original source directory. Keep this first profile in one source file
and put reusable Rust code in dependencies.

After adding or deliberately changing dependencies:

```sh
./bin/tellurc lock-native project/main.tellur
./bin/tellurc build project/main.tellur --release
```

Commit the resulting project lockfile. Normal builds use `cargo rustc --locked` and
never resolve a replacement lockfile. A missing or stale lock is a build error.
`lock-native` is the explicit dependency-resolution action. Cargo's usual cache,
registry and environment behavior applies; use `CARGO_NET_OFFLINE=true` when the
needed crates are already cached and network access is unwanted.

Dependencies accept a registry package name, `version`, `features` and
`default_features` (default true). Path/git dependencies and arbitrary Cargo tables
are outside this initial configuration. `serde` and `serde_json` are already
available to wrappers and cannot be overridden. The generated crate's lockfile
uses a per-output package name; the committed adapter lock uses the reserved
`tellur_adapter_lock` name. The compiler translates only that root identity when
building another output or expanded program. It preserves the dependency graph.

Native programs validate dynamic arguments/results against the manifest around
dispatch. Reference execution remains scripted. Native `--scenario` execution uses
the same scripts and does not call Rust adapters, making deterministic failure
injection possible alongside live host integration tests.

## Maintained application

The [settings application](../applications/settings/README.md) is the first sustained
consumer. Tellur handles defaults, key validation, updates and outcome mapping;
Rust supplies bounded reads, strict JSON decoding, encoding and file replacement.
See [adapter tests](../tests/rust_adapters.rs) and
[application scenarios](../tests/settings_application.rs). HTTP, retained host handles
and async operation/cancellation contracts remain later checkpoints.
