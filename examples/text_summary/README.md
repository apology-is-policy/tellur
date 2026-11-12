# Text summary through a Rust adapter

This example counts Unicode scalar values, text lines and whitespace-separated
words. Word counts preserve case and punctuation, and use sorted string keys.
Blank input produces `blank`; a count that cannot fit in `u32` produces `too_large`.
Rust supplies the text algorithm and Tellur declares the outcomes and exit status.

From the repository root with the installed compiler:

```sh
./bin/tellurc check examples/text_summary/main.tellur
./bin/tellurc build examples/text_summary/main.tellur --backend llvm
./examples/text_summary/build/main --json -- 'hello 😀 hello'
./examples/text_summary/build/main --json -- ''
```

The first invocation selects `measured` with 13 characters, one line and word counts
`hello: 2` and `😀: 1`. Both `measured` and `blank` exit with status 0; `too_large`
exits with status 1. Replace `--backend llvm` with `--backend rust` to compare the
same program through the default backend. LLVM currently requires macOS arm64.

`host.rs` and the explicitly declared `words.rs` module compile into a typed adapter
archive. Borrowed text lives only for the call; the returned map is owned data.
The private C bridge copies values between that archive and Tellur's precompiled
runtime. This example uses only the synchronous data profile admitted in 0.273.
Opaque resources and polled adapters still require the Rust backend. The supplied
lock is used with `--locked`; dependency changes require an explicit `lock-native`.
