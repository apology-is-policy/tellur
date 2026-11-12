# Struct constructors

[Language reference](../language-reference.md) · [Owning expressions](ownership-expressions.md)

Compiler 0.67 adds declared constructors; 0.69 extends them to reference-bearing
structs with explicit result-origin mappings. A constructor is an
initialization procedure: `type(arguments)` evaluates it and produces a fully
initialized value. It has no source outcome labels or explicit `exit` statement.
Expected alternatives such as invalid input still belong to ordinary factory
controls with explicit outcomes.

Since 0.96, independent constructor-definition errors within one parsed file can
report together before generated controls are registered. This includes invalid
`self` accesses and forbidden exits. Loading still stops at the first failing file;
see [constructor preparation diagnostics](diagnostics-and-limits.md#constructor-definition-preparation-batches).

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
Since 0.91, inaccessible calls in separate bodies can report together during
[constructor-call preparation](diagnostics-and-limits.md#constructor-call-preparation-batches).
This leaves later signature and ownership checks blocked until access is repaired.
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

Since 0.247, the opt-in [LLVM backend](llvm-backend.md#declared-constructors)
supports synchronous declared constructors, including nested and borrowed results.
Task forms remain gated; source initialization and origin
rules are the same as on the default Rust backend.

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

## Composing borrowed storage

Since 0.70, raw record initializers and variant, option and named-outcome payloads
can contain declared constructor calls even when the enclosing value stores
references. The two forms compose directly: braces assemble fields, while
parentheses run a declared constructor.

<!-- tellur:check -->
```tellur
struct text_view['a] {
    ref string text <- 'a;

    constructor(ref string input)[input] {
        self.text = input;
    }
}

struct message['a] {
    option<text_view>['a] body;
}

control main() exits done {
    mutable string owner = "borrowed payload";
    auto value = message { some { text_view(borrow owner) } };
    delta value.body {
        some { auto body } => present,
        none => absent
    }

    [present]: emit(body.text);
    [entry]:
        owner = "released";
        exit done;
}
```

Named fields evaluate in written order, even when that differs from declaration
order. Earlier values are retained before later constructors run. If a later
initializer faults, earlier resources still need their declared fault disposition;
no partial aggregate is delivered. Once assembly completes, the resulting value
owns those resources and carries the contained loans.

Borrow conflicts are checked across the initializer list. A constructor cannot
mutate a source retained by an earlier field. Shared and exclusive references keep
their ordinary permissions, tagged payloads are accessed only on their selected
paths, and each field retains its sources until its last use. This does not extend
a temporary owner's lifetime or allow a reference to escape local storage.

Since 0.72, [initializer borrow lifetimes](#initializer-borrow-lifetimes) distinguish
a completed call's temporary accesses from references carried by its result.
Mutation-then-borrow no longer requires a separate declaration solely to end the
completed constructor's nonescaping borrow.

The same composition works inside constructor field initialization, ordinary
arguments, control results, captures and selected adapter arms. Nested generic
types and imported constructors retain their existing visibility and origin rules.

## Initializer borrow lifetimes

Initializers run from left to right in written order, including named fields whose
order differs from the declaration. Since 0.72, a completed nested constructor's
nonescaping call borrows end before the next initializer. Its resulting value is
already retained: an owned snapshot keeps its earlier contents, and any returned
reference continues protecting its external source.

<!-- tellur:check -->
```tellur
struct text_change {
    string previous;

    constructor(ref mutable string text, string next) {
        self.previous = clone(text);
        deref text = move next;
    }
}

struct change_log['latest] {
    text_change first;
    text_change second;
    ref string current <- 'latest;
}

control main() exits done {
    mutable string owner = "before";
    auto changes = change_log {
        first: text_change(borrow mutable owner, "first"),
        second: text_change(borrow mutable owner, "second"),
        current: borrow owner
    };
    emit(move changes.first.previous);
    emit(move changes.second.previous);
    emit(changes.current);
    owner = "released";
    exit done;
}
```

This prints `before`, `first`, then `second`. Both exclusive calls have completed
before `current` borrows `owner`. The final shared borrow remains live until its
last use. A constructor that returns a reference to one input while only temporarily
accessing another keeps only the returned source borrowed.

An earlier reference argument or payload is held while later initializers run.
Moving its source, mutating through a conflicting alias, or reading through an
already-exclusive argument is rejected. Calls nested inside one initializer check
their own argument separation. Existing caller loans and outer pending arguments
cannot be retired by an inner initializer list.

<!-- tellur:reject E_ALIAS -->
```tellur
struct change {
    bool done;

    constructor(ref mutable string text) {
        deref text = "changed";
        self.done = true;
    }
}

struct held['a] {
    ref string text <- 'a;
    change later;
}

control main() exits done {
    mutable string owner = "before";
    auto value = held { borrow owner, change(borrow mutable owner) };
    emit(value.text);
    exit done;
}
```

These rules apply to nested constructor/control arguments, raw records, owning
collection literals, supported local shared-reference arrays/vectors, variant/outcome
payloads, exits, captures and selected adapter arms.
Extracting an owned field from an anonymous reference-bearing result also releases
its unused anonymous loans before the next initializer; selecting a reference keeps
that reference's dependencies. This does not relax temporary-owner escape checks,
borrow-mode requirements or completion obligations in unselected fields.

Loan retirement is a static permission change, not a destructor call or resource
completion. Earlier owned values remain pending until the entire list is delivered.
If a later initializer faults, their declared fault dispositions still apply.
Source-level named references retain the existing statement/field last-use rules;
this checkpoint does not add general reference rebinding or arbitrary expression
lifetimes.

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

## Projecting fresh results

Since 0.71, a field can be selected directly from a fresh reference-bearing
constructor result or raw record. Chained fields use ordinary transparent reference
access. The constructor runs once, at that expression's place in evaluation order.
Only a selected adapter arm runs its expressions.

<!-- tellur:check -->
```tellur
struct text_edit['source] {
    ref mutable string text <- 'source;

    constructor(ref mutable string input)[input] {
        self.text = input;
    }
}

control main() exits done {
    mutable string owner = "before";
    ref mutable auto text = text_edit(borrow mutable owner).text;
    deref text = "after";
    emit(text);
    owner = "released";
    exit done;
}
```

The selected reference retains the external `owner`; it does not borrow the
anonymous record's storage. Shared references stay shared, and exclusive references
transfer their existing permission. Unselected reference fields do not prolong
unrelated loans. A nested record, option, variant or outcome field carries its own
contained origins. Selected owned fields transfer without an implicit clone.
Returned references still need the ordinary public origin contract.

Projection cannot silently abandon completion obligations. At each owned step in a
field chain, an unselected resource or named-outcome field requires you to bind the
whole result first and explicitly dispose of its obligations. Selecting that
resource or outcome itself is allowed when no sibling obligation is abandoned.
References to external records do not make their other fields owned by the
temporary. Existing checks still forbid moving owned values out of borrowed storage.
Fault dispositions remain required for live resources before and after projection.

<!-- tellur:reject E_TEMPORARY -->
```tellur
outcome completion { done }
struct pending['a] {
    ref string text <- 'a;
    completion work;

    constructor(ref string input)[input] {
        self.text = input;
        self.work = done {};
    }
}

control main() exits done {
    string owner = "must handle work too";
    auto text = pending(borrow owner).text;
    emit(text);
    exit done;
}
```

This does not make the fresh record's own storage an escaping borrow source.
For example, `auto text = borrow mixed(borrow owner).owned;` cannot retain a
reference to an owned string inside that temporary. Move the string into a named
local first, then borrow the local. Borrowing the whole reference-bearing temporary
still requires a named owner. These rules do not promise immediate destruction of
anonymous storage, zero allocations or general physical copy elision.

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

Raw construction and ordinary factory controls remain available. Bind a constructed
reference-bearing result before borrowing the whole result, or when unselected
fields have completion obligations. Constructors do not add
overloads, default arguments, delegation, custom destructors, inheritance or
implicit conversions.

VS Code highlights `constructor`, shows it beneath its struct in the outline, and
provides its signature, origin mapping and initialization rules on hover. Hovering
over a constructor call also shows its checked input types, result type, ordered
origin groups and fault/contingency permission, including concrete generic calls
and imported constructors. The signature keeps the authored parameter names and
uses the current unsaved defining source. Private fields are not displayed. Invalid
source suppresses the call contract until repaired. Since 0.90, calls inside generic
controls and constructors show symbolic input types, including nested and
associated-type arguments. They preserve shared/exclusive borrows, ordered origin
groups and checked permissions independently of concrete callers. Try
[generic_construction.tellur](../../playground/generic_construction.tellur). Hovering `auto` reports
the inferred constructed type or a constructor-local binding's type. Missing-field
and ownership diagnostics update for unsaved source. Formatting preserves flat
path labels and wraps constructor argument lists using the normal width settings.

The [settings server](../../applications/settings/server.tellur) uses a constructor
to prepare an owned request context before its fault boundary. Its public interface
and request recovery behavior are unchanged.

Try [construction.tellur](../../playground/construction.tellur),
[borrowed_construction.tellur](../../playground/borrowed_construction.tellur) and
[composed_construction.tellur](../../playground/composed_construction.tellur), plus
[projected_construction.tellur](../../playground/projected_construction.tellur) and
[sequential_initializers.tellur](../../playground/sequential_initializers.tellur).
Compiler/runtime checks are in [constructors.rs](../../tests/constructors.rs) and
[borrowed_constructors.rs](../../tests/borrowed_constructors.rs), with composition in
[borrowed_composition.rs](../../tests/borrowed_composition.rs) and fresh projections in
[borrowed_projections.rs](../../tests/borrowed_projections.rs). Sequential access checks
are covered by [initializer_accesses.rs](../../tests/initializer_accesses.rs); actual retained-owner
fault cleanup is checked in [rust_adapters.rs](../../tests/rust_adapters.rs).


The LLVM runtime's [retained constructor bridge](llvm-backend.md#retained-constructor-runtime-bridge-0268)
now preserves caller destinations while a constructor's child suspends, including
borrowed results and local fault recovery. This is private runtime groundwork;
LLVM source admission for those calls still awaits registration and emission.
Constructors remain ordinary destination calls, not async task producers.


Compiler 0.269 completes LLVM source admission for ordinary constructors whose
children suspend. See [retained constructor source delivery](llvm-backend.md#retained-constructor-source-delivery-0269).
Caller destinations and declared result origins survive yields and host waits,
including construction inside cold/spawned parents. Constructor faults propagate
to caller supervision: constructors themselves cannot contain isolation/recovery
or become async task producers. Existing resource completion rules still apply.
The private runtime tests in 0.268 do not authorize additional constructor syntax.
