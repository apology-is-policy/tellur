# Formatting Tellur source

[Language reference](../language-reference.md) · [Path selection](../language-reference.md#labels-and-optional-grouping)

Compiler 0.56 provides a parser-directed formatter for modern `.tellur` source and
the supported `.path` compatibility syntax. Formatting changes whitespace only.
It never inserts or removes braces, path labels, semicolons, commas or parentheses,
and never rewrites a literal or migrates compatibility spellings.

## Terminal

Run these commands from the compiler checkout:

```sh
./bin/tellurc format playground/path_groups.tellur
./bin/tellurc format playground/path_groups.tellur --check
./bin/tellurc format playground/path_groups.tellur --write
```

The default prints the formatted source to standard output. `--check` writes no
source and returns status 0 when the file already matches the formatter, or 1 when
it needs formatting. Errors also return a nonzero status and explain the problem
on standard error. `--write` explicitly updates that file, skipping the write when
it is unchanged. `--check` and `--write` cannot be combined.

The command accepts one UTF-8 source file. It does not discover a manifest, load
imports, execute primitives or build native code. Syntax errors stop formatting
before any write; unresolved names, missing imports and type errors do not prevent
formatting otherwise valid syntax. Empty and comment-only documents are accepted.

## VS Code

Use **Format Document** from the Command Palette or the editor context menu. The
Tellur language server formats the current unsaved buffer, including untitled
Tellur documents. The edit uses the editor's spaces/tabs preference and indentation
width (1–16); the terminal uses four spaces. The extension must be running in a
trusted workspace, but formatting itself needs no manifest or Rust toolchain.

The formatter returns no edit for unchanged documents or broken syntax. Existing
syntax diagnostics explain the latter. Formatting does not save the file. Normal
editor undo works; format-on-save is an optional VS Code setting and is not enabled
by the extension. If another extension also offers formatting, select **Tellur
Language (Experimental)** in **Format Document With…**.

## Layout and preservation

Flat labels remain aligned with their containing body's ordinary statements.
Their selected statements gain one indentation level. Optional groups keep their
braces; code after a group returns to the enclosing body's indentation. Inline
adapter labels remain within their arms.

<!-- tellur:check -->
```tellur
control describe(bool available) exits done {
    delta available {
        true => present,
        false => absent
    }
    [present]: {
        emit("Available");
    }
    [absent]: {
        emit("Unavailable");
    }
    emit("Description complete");
    exit done;
}
```

The formatter puts statements, structural bodies and delta arms on separate lines.
Short payloads and constructors keep spaces inside their braces, such as
`found { value }`; empty braces remain `{}`. Generic arguments and origin selectors
stay compact, such as `vector<string>` and `input['a]`, while comparison operators
receive surrounding spaces. Existing wrapping in headers, arguments and payloads
is retained; there is no automatic line-width wrapping or column alignment yet.

Comments retain their spelling and order, including trailing spaces. A trailing
`//` comment stays after its preceding token; standalone comments stay between
the same tokens. Strings retain their exact bytes, including escapes and embedded
newlines. The first newline outside a literal chooses LF or CRLF for generated
layout; LF is the default when none exists. Paragraph breaks retain at most one
blank line. Nonempty output ends in one newline; whitespace-only input becomes
empty output.

Before returning output, the formatter checks the complete sequence of raw tokens
and comments against the input and reparses the result. This protects selection
extent and prevents adjacent tokens from accidentally becoming a different
operator. It does not prove program correctness: ordinary checking still verifies
types, ownership, resources and coverage.

This checkpoint supports whole-document formatting. Range formatting, on-type
formatting, configurable line widths and project-wide file discovery remain later
work. Existing files are not reformatted automatically when the compiler is updated.
