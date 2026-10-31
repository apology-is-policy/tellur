# Owning expressions and temporary arguments

[Language reference](../language-reference.md) · [References](../language-reference.md#references-borrowed-calls-and-lifetimes)

## Explicit moves

`move value` makes the existing ownership transfer explicit. Owning parameters,
initializers, record fields and outcome payloads already consume non-Copy values;
the keyword does not introduce a second kind of reference. Copy values, including
shared references, retain their Copy behavior. Exclusive references transfer their
permission and make the old reference unavailable.

<!-- tellur:check -->
```tellur
struct image {
    string caption;
    bytes pixels;
}

control identity<t>(t value) exits ready... {
    exit ready { move value };
}

control main() exits done {
    string caption = "sample";
    delta identity<image>(image(move caption, bytes "010203"));

    [ready]: emit(move value.caption); exit done;
}
```

Nested owning constructors need no intermediate source variable. `move` never
clones or grants permission to move through a borrow. Outstanding loans, partial
moves and resource obligations retain their ordinary checks. Use `clone(borrow x)`
when duplication is intended. `move` is an owning expression; it is rejected in
read-only contexts such as `clone(borrow move x)`.

A reference parameter normally reborrows its argument for the call. Passing
`move reference` explicitly transfers an existing exclusive reference instead;
the caller cannot use that reference again. The callee can return its loan under
its declared origin contract. Moving a shared reference copies its permission.

Generic controls preserve the ownership mode declared by their signature: `t`
owns, `ref t` shares, and `ref mutable t` borrows exclusively. This is not C++
forwarding-reference deduction; reference types as type arguments remain outside
the current generic profile. A generic owning wrapper needs no clone bound simply
to pass its input onwards.

## Borrowing a temporary for a call

<!-- tellur:check -->
```tellur
control display(ref string text) exits done {
    emit(text);
    exit done;
}

control normalize(ref mutable string text) exits done {
    deref text = "normalized";
    display(text);
    exit done;
}

control main() exits done {
    display(borrow "temporary");
    normalize(borrow mutable "before");
    exit done;
}
```

For an ordinary control or primitive call, a borrowed rvalue receives anonymous
owner storage for that invocation. Arguments retain written evaluation order, and
storage is released after the call, before its outcome continuation. A fault retires
that storage with the failed invocation. Evaluation remains path-selected.

This first profile requires an ordinary, reference-free temporary type. A resource
or reference-bearing aggregate needs a named owner. A call whose result contract
can borrow the temporary also needs a named owner, even if the caller intends to
ignore that alternative. Local `ref` declarations do not extend temporary lifetimes.

Moves express semantic ownership. They do not promise that every physical byte
transfer is elided. The current native backend moves its runtime values without
an implicit language-level clone; destination construction and the eventual native
ABI remain separate optimization work.
