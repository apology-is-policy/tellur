# File modules and visibility

Compiler 0.18 supports explicit file imports and private-by-default declarations.
Each file is one module. Imports are relative to the importing file, not the shell's
working directory. They introduce a namespace rather than injecting unqualified
names into the caller.

```tellur
import "people.tellur" as people;

control main() exits done {
    delta people.create("Ada", 37) { ready { people.person value } => created }
    [created]: delta people.name(borrow value) { found { ref string text } => named }
    [named]: emit(text);
    [named]: emit(u32_text(value.age));
    [named]: exit done;
}
```

`people.tellur` defines the public interface and private representation:

```tellur
pub struct person {
    string name;
    pub u32 age;
}

pub control create(string name, u32 age) exits ready { person value } {
    exit ready { person { name, age } };
}

pub control name(ref person value) exits found { ref string text from value } {
    exit found { borrow value.name };
}
```

From the project root:

```sh
./bin/tellurc check playground/modules/main.tellur
./bin/tellurc exec playground/modules/main.tellur
./bin/tellurc inspect playground/modules/main.tellur playground/manifest.json main --expanded
```

The entry file discovers its manifest in the usual way. That manifest supplies
foreign types and primitives for the whole module graph; imported files do not
silently select additional manifests.

## Visibility and identity

- `pub` exports a control, struct, variant or outcome. Other declarations are private
  to their file. An entry control may remain private: `main` is selected explicitly
  by the compiler/runtime entry mechanism.
- Struct fields are private unless marked `pub`. A public struct with private fields
  must be constructed through a public control. Reading, borrowing, assigning and
  projecting private fields from another module are rejected. Its defining module
  retains normal field access and construction.
- Exporting a variant or outcome exports its alternatives and payload shapes. Public
  parameters, exits, public fields and variant/outcome payloads cannot expose private
  named types. Since 0.44, public controls must declare explicit exit labels, payload
  types and borrowed-result origins; a public named outcome contract is also supported.
  `exits auto` and `label...` are reserved for private controls.
  Use a public named contract rather than exporting producer-indexed outcome schemas.
- Imports must be qualified: `people.person`, `people.create(...)`, or
  `people.choice.present { ... }`. Unqualified imported constructor tags do not enter
  the caller's namespace or shadow builtin option constructors.
- Multiple aliases and symlink paths for the same canonical file share one module
  identity within the compilation graph. Separate files retain distinct nominal types
  even when declaration names and field layouts match. Diamond dependencies load once.
- Import namespaces cannot collide with declarations, other aliases or local values
  in that module. The `_path_m_` prefix is reserved for compiler identities.
- Imports are nonempty relative file paths. Cycles and missing files are diagnostics.
  A graph is limited to 256 files and 8 MiB of source; exceeding a limit fails instead
  of compiling an incomplete graph.

There are no wildcard imports, reexports, directory-module conventions, package
search paths or separate binary compilation yet. All loaded modules are checked,
including private or currently uncalled controls. Ordinary ownership and completion
obligations cross module boundaries unchanged. Private representation is a source
access rule, not a promise that inspection or generated code conceals implementation.

## Diagnostics, editor and provenance

VS Code uses unsaved buffers for open dependencies and disk contents for closed ones.
Changing a dependency rechecks open callers. Errors originating in an imported file
appear on the caller's import with a related location pointing to the actual file
and line. Outcome mismatches through direct forwarding and missing direct-call arms
also include related producer/forwarder locations, mapped to their original files
and open buffers. Closing an unsaved dependency restores the disk version for subsequent
checks. Tellur and JSON file watchers invalidate diagnostics for disk changes inside
the workspace. Dependencies outside watched workspace folders are read on the next
check; automatic external-directory watching is not implemented.

Root declaration names remain unchanged. Imported declarations have internal
identities, with original names and canonical file paths retained in
`CheckedProgram::source_files()`. `inspect` appends that map for a multi-file program.
Graph spans use combined source coordinates; the map translates them to original
files. Certified expansion retains this information. Internal identity spellings are
not a stable ABI or a syntax to paste into source.

The library API is `compile_file(path, manifest)`. Editors can use
`modules::compile_with_loader` to provide unsaved source text for canonical paths.
Import targets must exist on disk to establish identity; an entry buffer may be new.
`compile(source, manifest)` remains useful for one-file buffers and explicitly rejects
imports because it has no source-directory context.

`compare`, protocol analysis, inspection, native builds and expansion certificates
load the full graph through the CLI. `extract-tail` remains a single-file source edit
and rejects imports. A future source-level lift/extract must preserve the defining
module's access rights and provenance; copying private field expressions directly
into a caller is not a valid refactoring.

Generic data (0.20) uses the same namespace, for example `data.box<string>`. A
specialization retains the defining file's identity and field visibility. Public
interfaces may not hide private type arguments inside a generic application. Generic
variant constructors are qualified (`data.choice<u32>.some { 1 }`); their short tags
remain available in a delta pattern for a known input type.

Compiler 0.37 also checks the nominal completion contract named by a public
control, not only its individual payload types. A private outcome declaration
cannot appear in a public `exits` signature; make the contract public first.
