# Extract a control tail

Compiler 0.16 adds `extract-tail`: a conservative source refactoring that moves an
unconditional tail into a new control. It infers typed inputs, writes an explicit
exit contract, replaces the tail with `forward`, and checks the complete result.
The command prints the candidate; it never edits the original file.

From the project root:

```sh
# Line 12 starts the assignment in greeting.
./bin/tellurc extract-tail playground/extraction/original.tellur playground/manifest.json \
  greeting 12 finish_greeting > /tmp/path-extracted.tellur

./bin/tellurc check /tmp/path-extracted.tellur playground/manifest.json
./bin/tellurc compare playground/extraction/original.tellur playground/manifest.json \
  /tmp/path-extracted.tellur playground/extraction/scenarios.json
./bin/tellurc exec /tmp/path-extracted.tellur playground/manifest.json
```

Redirect to a different file: shell redirection to the input filename would truncate
it before the compiler could read it. The checked-in
[extracted example](../playground/extraction/extracted.tellur) is the generated result;
both example files can also be opened and run in VS Code.

The original tail is:

```tellur
control greeting(bool available) exits greeting_result {
    emit("Preparing greeting");
    mutable string text = "Hello";
    text = concat(text, ", Tellur!");
    exit delta available {
        true => ready {text},
        false => empty {}
    };
}
```

Extraction produces:

```tellur
control greeting(bool available) exits greeting_result {
    emit("Preparing greeting");
    mutable string text = "Hello";
    [entry]: forward finish_greeting(available, text);
}

control finish_greeting(bool available, string extracted_text) exits greeting_result {
    mutable string text = extracted_text;
    text = concat(text, ", Tellur!");
    exit delta available {
        true => ready {text},
        false => empty {}
    };
}
```

The `greeting_result` declaration remains in the compilation unit. Mutable inputs
move into an explicit mutable local, with a fresh parameter name. Nothing is
implicitly cloned. Reference parameters retain reference types. When the original
contract is inferred or inline, the helper receives an explicit inline contract
with the checked labels and payload types.

## Selection and boundary rules

- Select the one-based line of a top-level statement on `entry`, or a whole top-level
  loop or path alias. The line must begin with that item, optionally `[entry]:`.
  Multiple statement starts on the same line are rejected. The closing control brace
  must be on its own line. Historical `region` syntax is not supported.
- The selection extends through the end of the control. Nested adapters, local path
  names and complete loops within that tail are copied. An arbitrary middle range,
  a branch-local start, and cuts through a loop are unsupported.
- Values used but not declared inside the tail become inputs, in name order. Each
  needs one recoverable checked type. A stored anonymous inferred outcome needs a
  named outcome contract before it can cross this generated boundary.
- Earlier path facts are not silently captured. A tail that uses those facts fails
  the new control's path check; move the cut before their defining delta or alias.
- The ordinary checker validates ownership, loans, returned-borrow origins,
  resource completion and every declared exit. Passing a partially moved aggregate
  as a whole is rejected. Some local borrow aliases have no retained storage slot;
  move the cut before their declaration so their owner and borrow are inside the tail.
- A new snake_case helper name must not collide with a control, type or primitive.
  Generated names and declarations also pass the ordinary parser and checker.

The copied tail retains its exact text, including its comments and named paths.
Text outside the tail is retained, except for the inserted forwarding statement and
appended helper. Comments on preceding lines remain with the caller. Generated
lines use LF; copied CRLF lines retain CRLF.

Add `--report` to receive JSON instead of source text:

```sh
./bin/tellurc extract-tail playground/extraction/original.tellur playground/manifest.json \
  greeting 12 finish_greeting --report
```

The report contains `source`, `control`, `helper`, `start_line`, `helper_line`,
`inputs` (name, internal checked type and mutable-local flag), and the claim
`rechecked_tail_extraction_preserving_public_contracts`. Library users can call
`extract::extract_tail` with the same source, manifest and selection.

## What the check establishes

Both the original and the whole candidate compile. Every preexisting control keeps
its public parameter descriptors, checked exit contract and returned-borrow summary.
A failing candidate produces `E_EXTRACT` with the underlying diagnostic; no partial
candidate is printed. Hidden path dependencies and ownership errors are therefore
reviewable failures rather than implicit extra inputs.

This is **not a proof of behavioral equivalence** and not an expansion certificate.
The new source has new locations and graph identities; no source-to-source
provenance correspondence is emitted. Its later graph expansion independently uses
the existing [expansion certificate validator](expansion-certificates.md).
Finite scenario comparison supplies additional behavioral evidence. Tests exercise
owned/mutable values, references, resources, named outcomes, local paths and loops,
and compare interpreted/native execution at O0/O2 for the runnable example.

The general inverse `expand(extract(G)) ≃ G` remains a research obligation.
Arbitrary graph cuts need explicit live-outs, continuation boundaries, external
path interfaces and a checked provenance correspondence. This initial tail-only
operation avoids those interfaces by returning directly through the original
control's public contract. There is no `extract` source keyword or editor code action.

Generic control templates are not yet eligible for source extraction. A non-generic
caller may contain explicit generic calls; the generated helper and whole program
are rechecked as usual. Generic declaration names are reserved against helper-name collisions.
