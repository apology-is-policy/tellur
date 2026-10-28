# Rust source adapters

Compiler 0.45 can compile project-supplied Rust wrappers with a generated native
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
Returned references, exclusive foreign mutation, opaque retained resources, nominal
records/sums, callbacks and asynchronous adapters are not supported by this profile.

The current bridge converts runtime values and may copy borrowed input data. It
does not promise zero-copy FFI. The generated function types prevent ordinary safe
wrappers from returning borrowed input as an owning result. Arbitrary Rust remains
a trust boundary: types do not prove effect declarations, error classification,
blocking behavior or domain laws. An adapter panic terminates the native execution;
it is not converted into a recoverable Tellur alternative. No unwinding/abort or
cancellation cleanup guarantee is implied.

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
