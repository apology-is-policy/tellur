# Identity routing and grouped delta arms

[Language reference](../language-reference.md) · [Revision](../control-flow-revision.md)

Compiler 0.64 supports flat deltas that retain the producer's outcome names.

## Importing outcomes as paths

<!-- tellur:check -->
```tellur
control read_message() exits ready { string text }, absent {
    exit ready { "hello" };
}

control main() exits done {
    delta read_message();

    [ready]: emit(text); exit done;
    [absent]: exit done;
}
```

`delta read_message();` imports `ready` and its `text` binding, and `absent` without
a payload. It does not return either outcome from the caller. Payloads transfer
or reborrow exactly as in an explicit pattern; no cloning is inserted.

Payload names in the producer's contract become names the caller can depend on.
In inferred contracts, a simple named payload preserves that name when all exit
sites agree; expressions and inconsistent names use `value_0`, `value_1`, etc.
Use explicit payload declarations when publishing stable names matters.

Omitted alternatives in a braced flat delta also use identity routing. Duplicate
and unknown alternatives remain errors. All resulting histories still need an
exit, restart or fault disposition; missing dispositions report coverage and
available contributor evidence. A common `[entry]` disposition can intentionally
handle every imported history.

Inline outcome adapters (`exit delta ...` and `auto value = delta ...`) remain
explicit and exhaustive. They construct outcomes, whereas flat deltas create
paths. Legacy `region` decisions keep their explicit-arm requirement.

## Grouped arm statements

<!-- tellur:check -->
```tellur
control request(bool image) exits has_image { string data }, has_message { string text } {
    exit delta image {
        true => has_image { "pixels" },
        false => has_message { "message" }
    };
}

control main() exits done {
    delta request(true) {
        has_image { auto data } => {
            string img = data;
        }
    }

    [has_image]: emit(img); exit done;
    [has_message]: emit(text); exit done;
}
```

An arm ending in a statement group creates a path with the incoming outcome's
name. Its statements run only on that path. Like grouped path labels, these
braces end the selection without introducing a lexical value scope: `img` remains
available on `[has_image]`. The moved `data` is unavailable there afterward.

The original outcome and contributor provenance remain unchanged. Creating `img`
does not change the producer's payload contract. Use an outcome adapter when
constructing a different first-class result. A grouped arm may also exit, fault,
restart a loop, invoke controls, or contain further decisions; ordinary coverage,
resource and lifetime rules apply throughout.

## Qualified decision paths

<!-- tellur:check -->
```tellur
control choose(bool flag) exits yes, no {
    exit delta flag { true => yes {}, false => no {} };
}

control main() exits done {
    first = delta choose(true);
    second = delta choose(false);

    [first.yes & second.no]: emit("selected"); exit done;
    [first.no | second.yes]: exit done;
}
```

A named flat decision exposes `site.path` for every continuing arm. A short path
remains usable while unambiguous. After two named decisions introduce the same
short name, qualify it instead. Earlier resolved uses and explicit path aliases
retain their original selection.

Qualification is for paths, not an implicit payload namespace. Repeated payload
names on overlapping histories need explicit binding names, for example
`ready { auto first_text } => ready`. Different types on disjoint histories still
require distinct binding names under the existing value rules.
