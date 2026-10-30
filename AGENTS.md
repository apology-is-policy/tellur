# Tellur source style

Use `applications/settings/server.tellur` and `docs/reference/formatting.md` as the
style reference for new or edited Tellur code. The language author's preference is:

- Keep path labels visually distinct and unbraced unless grouping helps understanding.
- Keep short dispositions beside their labels, for example `[missing]: exit absent;`.
- For a label on its own line, indent its selected statements beneath it.
- Wrap long signatures, calls and payloads into readable multiline lists. The
  formatter's default target is 100 columns with four spaces.
- Separate decisions from their final path dispositions with a blank line when it
  helps scanning. Retain meaningful comments and paragraph breaks.

Run `./bin/tellurc format FILE --write` on new or substantially edited Tellur files
using the current compiler. Preview the result when preserving a user's manual
layout matters. Avoid unrelated repository-wide reformatting. Rust source uses
`cargo fmt`. Formatting never adds or removes path-grouping braces.
