# Lexical structure and precedence

[Language reference](../language-reference.md) · [Core grammar](grammar.md)

This chapter describes modern source accepted by compiler 0.44.0. Compatibility
spellings are listed separately in the main reference.

## Source and identifiers

Source files contain UTF-8 text. Identifiers are case-sensitive ASCII names matching
`[A-Za-z_][A-Za-z0-9_]*`. Use `snake_case` for controls, types, associated types,
fields, values and path atoms. `string` and `String` are different names; current
source uses the lowercase builtin types. Unicode is allowed in string contents,
but not in ordinary identifiers. A leading apostrophe introduces an origin-group
name; it is not part of an ordinary value identifier.

Compiler-generated names are reserved. In particular `_path_poly_` and `_path_m_`
are internal prefixes; do not use them for source declarations. `_` is not a
wildcard pattern: a delta must name every alternative explicitly.

Whitespace separates tokens and otherwise carries no statement-layout meaning.
Newlines do not insert semicolons. `//` begins a comment continuing to the next
newline or end of file. Block comments and nested comments are not supported.

Most keywords are recognized by their syntactic position rather than by a separate
lexical keyword token class. This is not a promise that a keyword can safely be used
as a declaration name: statement and expression parsers give special forms priority.
Avoid keyword names in user declarations. The main modern vocabulary is:

| Purpose | Words |
| --- | --- |
| Declarations and modules | `control`, `struct`, `variant`, `outcome`, `trait`, `implement`, `type`, `import`, `as`, `pub` |
| Completion and control flow | `exits`, `exit`, `delta`, `forward`, `capture`, `path`, `over`, `loop`, `again` |
| Types and access | `auto`, `mutable`, `ref`, `borrow`, `deref`, `from`, `self` |
| Capabilities and operations | `copy`, `clone`, `drop`, `emit` |
| Builtin values and constructors | `true`, `false`, `unit`, `some`, `none`, `bytes`, `map` |

Builtin collection operation names and manifest primitive names also participate in
name resolution; the table is a vocabulary guide, not a complete reserved-word list.
`entry` is the distinguished entry selector. `auto` is supported at selected inference
sites, not as a general unknown type in arbitrary signatures.

## Literals

| Kind | Syntax and restrictions |
| --- | --- |
| Boolean | `true` or `false` |
| Unsigned integer | Decimal digits, optionally followed by `u32`; range 0 through 4294967295 |
| String | Double quotes surrounding UTF-8 text, with the escapes below |
| Bytes | `bytes"00ff7a"`; an even number of hexadecimal digits, two per byte |
| Unit | `unit` |

Integer underscores, hexadecimal integer notation, floating-point literals, signed
integers and unary numeric negation are not implemented. `bytes"ff"` is a byte
sequence, not an integer. Byte hex digits may use either letter case. Since `bytes`
and the string are separate tokens, intervening whitespace is accepted too.

String escapes are exactly `\n`, `\r`, `\t`, `\\`, and `\"`. Other backslash
escapes are errors. Write Unicode characters directly; there is no `\u` escape or
string interpolation. Literal newlines inside quoted strings are accepted; using
`\n` often makes the intended source layout clearer.

<!-- tellur:body -->
```tellur
u32 limit = 4294967295u32;
string greeting = "Hello, světe!\n";
bytes signature = bytes"00Ff7a";
bool enabled = true;
unit nothing = unit;
```

Collection and record syntax constructs values rather than introducing new lexical
literal kinds. List literals need an expected array/vector type. `map {}` constructs
an empty map with an expected key/value type. `none` needs an expected option type;
`some { expression }` can infer its payload where the expression is unambiguous.

## Expression precedence

From tightest binding to loosest:

| Level | Forms | Association |
| --- | --- | --- |
| Primary and projection | Parentheses, literals, constructors, calls, `.field` | Projection chains left to right |
| Prefix | `borrow`, `borrow mutable`, `deref`, `!` | Prefixes compose right to left |
| Comparison | `==`, `!=`, `<`, `<=`, `>`, `>=` | One comparison; chains rejected |
| Conjunction | `&&` | Left associative, short-circuit |
| Disjunction | `\|\|` | Left associative, short-circuit |

Thus `deref input.text` means `deref (input.text)`, and `borrow deref input` means
`borrow (deref input)`. `!ready || count < limit && enabled` groups as
`(!ready) || ((count < limit) && enabled)`. Parentheses may make intent clearer.

<!-- tellur:body -->
```tellur
bool ready = false;
bool enabled = true;
u32 count = 2;
u32 limit = 5;
bool proceed = !ready || count < limit && enabled;
```

Equality supports bool, u32, string and unit. Ordering supports u32. There is no
implicit aggregate equality. Arithmetic uses checked control operations such as
`checked_add`; `+` is not a value-expression addition operator. A call-shaped
expression is not permission to invoke any source control in expression position:
only supported pure operations and designated builtins work there.

Path masks have their own precedence: parentheses, then `&`, then `|`.
Their operands are established path atoms or aliases, not boolean values. There
is no executable path complement. `&&`/`||` operate on bool; `&`/`|` combine paths.

## Punctuation by position

| Form | Position | Meaning |
| --- | --- | --- |
| `(arguments)` | After a callable name | Invoke with ordered arguments |
| `(expression)` | Within an expression | Group an expression |
| `{ type name }` | Exit contract or delta pattern | Describe a field or bind an incoming field |
| `{ values }` | After a constructor or exit label | Supply positional payloads |
| `{ field: value }` | Struct construction | Supply named fields in written evaluation order |
| `[path]:` | Control body | Select subsequent statements |
| `[path] over &: { ... }` | Control body | Lexical scope with intersected selections |
| `[values]` | Expression | Array/vector construction with an expected type |
| `['a, 'b]` | Struct/variant declaration | Declare ordered origin groups |
| `<- 'a` | Struct field or variant payload declaration | Assign that reference to an origin group |
| `[left, right]` | Grouped result type | Bind its groups to input origins |
| `input['a]` | Origin contract | Select an external origin carried by input |
| `<types>` | Type or generic callable | Explicit type arguments |
| `=>` | Delta arm | Map an alternative to a path or adapter result |
| `...` | Exit declaration after a label | Infer that label's fixed nonempty payload |
| `=` | Binding, assignment, associated type | Initialize, replace, or define a type according to context |
| `+` | Generic bound list | Require all listed capabilities/traits |
| `;` | Statement or field end | Terminate the item explicitly |

Braces on an inline outcome expression are mandatory even for a nullary result:
`empty {}`. A statement may use `exit empty;`. A flat delta's final semicolon is
optional; `exit delta ...;` and a declaration initialized with delta require one.
Trailing commas are accepted in parameter, payload, constructor and arm lists;
do not assume they are accepted in every angle-bracket or origin list.
