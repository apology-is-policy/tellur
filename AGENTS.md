# Villanelle source style

Use `applications/settings/server.vn` and `docs/reference/formatting.md` as the
style reference for new or edited Villanelle code. The language author's preference is:

- Keep path labels visually distinct and unbraced unless grouping helps understanding.
- Keep short dispositions beside their labels, for example `[missing]: exit absent;`.
- For a label on its own line, indent its selected statements beneath it.
- Wrap long signatures, calls and payloads into readable multiline lists. The
  formatter's default target is 100 columns with four spaces.
- Separate decisions from their final path dispositions with a blank line when it
  helps scanning. Retain meaningful comments and paragraph breaks.

Run `./bin/vnc format FILE --write` on new or substantially edited Villanelle files
using the current compiler. Preview the result when preserving a user's manual
layout matters. Avoid unrelated repository-wide reformatting. Rust source uses
`cargo fmt`. Formatting never adds or removes path-grouping braces.

## Native performance

Aim to match or beat equivalent optimized Rust for every operation, data structure,
algorithm, and memory-management path. Use the same observable work and verify the
results. Measure the hot operation separately from launch, decoding and output.
For a gap, inspect generated code and measure allocation count, requested bytes
and data movement as well as time. Record the workload, compiler settings, hardware,
remaining gap and next experiment. Preserve checked ownership, origins, outcomes,
resource completion, cancellation and fault diagnostics when optimizing.
