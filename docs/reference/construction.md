# Struct constructors

[Language reference](../language-reference.md) · [Owning expressions](ownership-expressions.md)

Compiler 0.67 adds declared constructors; 0.69 extends them to reference-bearing
structs with explicit result-origin mappings. A constructor is an
initialization procedure: `type(arguments)` evaluates it and produces a fully
initialized value. It has no source outcome labels or explicit `exit` statement.
Expected alternatives such as invalid input still belong to ordinary factory
controls with explicit outcomes.

<!-- tellur:check -->
```tellur
struct greeting {
    pub string text;

    constructor(string name, bool formal) {
        delta formal {
            true => formal,
            false => casual
        }

        [formal]: self.text = concat("Good day, ", move name);
        [casual]: self.text = concat("Hello, ", move name);
    }
}

control main() exits done {
    auto first = greeting("Ada", false);
    greeting second = greeting { text: "Welcome!" };
    emit(move first.text);
    emit(move second.text);
    exit done;
}
```

A declaration may appear among the struct's fields, including before them. Its
parameters are type-first, just like control parameters. The constructor has
access to the defining module's private fields. `self` is a reserved partial-record
name inside the constructor; it is not an ordinary reference parameter.

## Initialization and ownership

`self.field = expression;` initializes one complete field. Each field must be
initialized exactly once on every history that completes normally. Disjoint paths
can initialize the same field with different values. Initialization order follows
the executed statements; final record layout follows field declaration order.
No field is zeroed, defaulted or assumed initialized.

Initialized fields may be read or shared-borrowed using their ordinary types and
ownership rules. Reading before initialization, moving a field away without a
complete result, or overlapping initializations is rejected. In this first slice,
initialized fields cannot be reassigned or exclusively borrowed. Build and mutate
an ordinary local value first, then move it into its field. There is no access to
whole `self`, including borrowing, moving or passing it to a helper. This prevents
partially initialized records from escaping.

<!-- tellur:reject E_UNAVAILABLE -->
```tellur
struct incomplete {
    string text;
    constructor(bool initialize) {
        delta initialize { true => yes, false => no }
        [yes]: self.text = "present";
    }
}
```

Constructor parameters and ordinary locals retain their normal rules. Unused
must-complete resources must be completed or transferred; completing the constructor
does not abandon them. Falling through with all fields present produces the record.
An all-faulting or nonterminating constructor need not initialize fields on histories
that never complete. Tail loops retain their usual requirement to exit or restart;
constructors cannot use an ordinary `exit` to finish a loop or return a value.
Use an ordinary helper control for iterative preparation.

## Visibility and raw construction

Constructor visibility is independent of field visibility:

- `constructor(...)` is private to its module.
- `pub constructor(...)` permits callers that can name the struct to invoke it.
- `type { ... }` remains raw field construction. It is allowed inside the defining
  module, and outside it when every field is public—even when a constructor exists.

A constructor provides reusable initialization; its existence alone does not
prevent bypassing that procedure. Private fields establish that restriction.
Public contract snapshots attach a public constructor's parameters, result-origin
mapping, fault permission and contingency permission to its struct, separately from
raw constructibility. Private constructors are omitted.

## Calls, order and temporaries

Constructor calls nest in owning expressions, ordinary call arguments, collection
literals and exit payloads. Arguments and named initializers evaluate in written
order. An earlier argument is held as an owned temporary while later arguments
are evaluated. If a later constructor faults, those earlier values still have
their resource obligations. Ordinary `&&` and `||` short-circuit evaluation also
applies to constructor calls in their operands. Path-selected expressions execute
only on their selected histories.

A call may borrow a fresh constructed ordinary value for the call duration under
the existing [temporary-borrow restrictions](ownership-expressions.md#borrowing-a-temporary-for-a-call).
The temporary cannot contain resources or references, and no returned borrow may
escape it. `move` transfers ownership; it does not promise a particular machine
instruction sequence or physical copy elision.

Since 0.68, native constructors deliver completed values into caller storage
without an intermediate outcome tag or payload vector. Partial fields stay owned
by the constructor until initialization completes. A later caller fault releases
the completed value through the caller's frame. Selecting a field from an owned
temporary transfers that field without cloning its allocation. Actual records,
argument vectors and intermediate storage still exist; this is not a general
zero-allocation or copy-elision guarantee.

<!-- tellur:check -->
```tellur
struct box<t> {
    t value;
    constructor(t value) {
        self.value = move value;
    }
}

control main() exits done {
    auto wrapped = box<string>("No clone bound is needed to forward ownership.");
    emit(move wrapped.value);
    exit done;
}
```

Generic constructors use the struct's explicit type arguments. Data parameters
currently have no bounds, so generic bodies must work for unconstrained values:
borrowing and forwarding are allowed; copying or cloning requires a capability
that an unconstrained parameter does not provide.

## Reference-bearing results

A constructor that returns stored references declares their origins in brackets
after its parameter list. The entries correspond, in order, to the struct's origin
groups. They are a static lifetime contract, not additional runtime arguments.
Mappings are required for both private and public constructors.

<!-- tellur:check -->
```tellur
struct text_pair['a, 'b] {
    ref string left <- 'a;
    ref string right <- 'b;

    constructor(ref string left, ref string right)[left, right] {
        self.left = left;
        self.right = right;
    }
}

control main() exits done {
    mutable string first = "first";
    mutable string second = "second";
    auto pair = text_pair(borrow first, borrow second);

    emit(pair.left);
    first = "released";
    emit(pair.right);
    second = "released";
    exit done;
}
```

Here `'a` maps to the constructor parameter `left`, and `'b` to `right`. At the
call, those become borrows of `first` and `second`. Each field retains its own
source until its last use. Moving the result transfers its permissions; copying a
shared-reference record creates another retainer. An exclusive reference cannot
be duplicated or obtained from shared input authority.

The same rules work for `ref mutable` fields, nested records, optional/tagged
payloads and references to reference-bearing records. A mapping such as
`constructor(ref view input)[input, input['a]]` distinguishes the input record's
storage from an external origin carried inside it. It follows the existing
[public nested-reference contracts](../language-reference.md#public-nested-reference-contracts).
The body must establish every declared mapping; the annotation cannot authorize a
different source. Public contract snapshots include the constructor's mappings.

Field initialization remains path-sensitive. Disjoint paths may initialize a
reference field; every normally completing path must initialize it exactly once.
The runtime address follows the executed initializer, while checking retains the
possible source dependencies. An initialized mutable reference permits editing its
pointee. The field binding itself still cannot be reassigned.

References cannot escape constructor-local storage, an owned parameter's storage,
or another field of partial `self`. Owners passed by temporary borrow cannot
support a returned reference. A constructor may consume an existing reference
carrier and return its external origins through `input['a]`; consuming the carrier
does not extend the lifetime of its own storage.

<!-- tellur:reject E_LIFETIME -->
```tellur
struct view['a] {
    ref string text <- 'a;

    constructor(ref string input)[input] {
        string local = "cannot escape";
        self.text = borrow local;
    }
}
```

Declared constructors can initialize nested reference-bearing fields with other
constructor calls, and their results can pass through ordinary calls, exits,
captures and adapters. Existing origin-schema and generic restrictions still
apply; generic parameters do not acquire an unknown nested origin shape.

## Faults and contingency

A private constructor infers transitive fault permission. A public constructor
that may fault must declare `faults` after its parameter list and any result-origin
mapping. `@contingent` before `pub constructor` explicitly admits a public
constructor in contingency; its body
is checked against the same call restrictions as a contingent control. Private
contingency permission is inferred, as for private controls.

A source fault retires the constructor activation and propagates to the caller's
nearest boundary. Initialized fields remain individual owners until successful
assembly. Every live must-complete field, unconsumed input and earlier pending
argument needs its declared fault disposition. Uninitialized fields have no value
to release. Retained Rust owners with `fault: "release"` are released exactly once;
this is not an implicit call to an ordinary fallible cleanup control.

No partial value is delivered to contingency. Boundaries and recovery belong
outside the constructor in this slice; constructor-local isolation is rejected.
Faults never become construction outcomes. An enclosing boundary follows the
existing administrative recovery rules.

## Current boundaries and editor support

Raw construction and ordinary factory controls remain available. When a raw
reference-bearing record or sum initializer contains a declared constructor call,
bind the constructed value separately first. That broader composition is a separate
staging checkpoint. Also bind a constructed reference-bearing result before
projecting its fields or borrowing the whole result. Constructors do not add
overloads, default arguments, delegation, custom destructors, inheritance or
implicit conversions.

VS Code highlights `constructor`, shows it beneath its struct in the outline, and
provides its signature, origin mapping and initialization rules on hover. Hovering
`auto` reports the inferred constructed type or a constructor-local binding's type. Missing-field
and ownership diagnostics update for unsaved source. Formatting preserves flat
path labels and wraps constructor argument lists using the normal width settings.

The [settings server](../../applications/settings/server.tellur) uses a constructor
to prepare an owned request context before its fault boundary. Its public interface
and request recovery behavior are unchanged.

Try [construction.tellur](../../playground/construction.tellur) and
[borrowed_construction.tellur](../../playground/borrowed_construction.tellur).
Compiler/runtime checks are in [constructors.rs](../../tests/constructors.rs) and
[borrowed_constructors.rs](../../tests/borrowed_constructors.rs); actual retained-owner
fault cleanup is checked in [rust_adapters.rs](../../tests/rust_adapters.rs).
