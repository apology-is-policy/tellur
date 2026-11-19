# Syntax lab

Design material for the redesigned surface syntax. Nothing here compiles: the
files under `syntax/v2/` are written in the proposed word spelling, and
`rails/` holds the same programs in the **Rails** glyph form that the author
adopted on 19 November 2026. See [the redesign plan](../../docs/redesign-plan.md#55-adopted-surface-rails).

| Path | Contents |
| --- | --- |
| `rails/*.txt` | The six reference programs in the adopted Rails form. These are the reference for how v2 source looks. |
| `syntax/v2/*.vn` | The same programs in the word spelling, which remains the input form. |
| `syntax/v1/` | Verbatim extracts of the 0.306 sources they were derived from. |
| `glyphs/lab.js` | A small transpiler: parses the word spelling and prints a style (`notation`, `flow`, `rails`, `glyph`). |
| `syntax/build.js`, `glyphs/page.js`, `play/play.js` | Builders for the three comparison pages (before and after, glyph studies, the Rails playground). |

Regenerate a Rails rendering:

```sh
node experiments/syntax_lab/glyphs/lab.js experiments/syntax_lab/syntax/v2/read_file.vn rails
```

The page builders embed DejaVu Sans Mono, the one common monospace font that
covers every glyph in use, and expect it under `~/Library/Fonts` together with
the Python `fonttools` and `brotli` packages. `play/play.js` also expects the
Monaco 0.45.0 stylesheet and icon font beside it. The published pages are
private; rebuild them from here if they are needed again.
