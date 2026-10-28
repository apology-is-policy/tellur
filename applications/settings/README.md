# Settings service: CLI slice

This is Tellur's maintained application. It starts as a local JSON settings CLI;
the same Tellur module will later serve HTTP requests. There is no server yet.

`settings.tellur` owns defaults, key validation, lookup/update decisions and failure
mapping. `host.rs` implements file, JSON and text mechanisms through typed Rust
adapters generated from `manifest.json`. The committed `Cargo.lock` makes native
builds use the declared dependency graph.

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

The launcher prints the final outcome. Currently a normally completed outcome
still gives OS exit code 0, including application failure alternatives; execution
faults give a nonzero code. Use `--json` to inspect the exact outcome in automation.
Explicit process-status mapping is a separate launcher design decision.

In VS Code, open `main.tellur` and use **Tellur: Build and Run**. Set the entry and
arguments in workspace settings, for example:

```json
{
  "tellurLanguage.entryRegion": "set",
  "tellurLanguage.programArguments": ["/tmp/tellur-settings.json", "theme", "dark"]
}
```

Changing dependencies is explicit:

```sh
./target/release/tellurc lock-native applications/settings/main.tellur
```

[Adapter documentation](../../docs/rust-adapters.md) describes the generated Rust
interface and current type boundary. [Application tests](../../tests/settings_application.rs)
exercise live persistence/reload, invalid requests/files, size limits, failed writes,
uncertain replacement and cleanup details. Scripted failures agree with interpreted
and expanded execution, including native O0/O2 builds.
