# Core grammar

[Language reference](../language-reference.md) · [Lexical structure](lexical-structure.md)

This is a reading grammar for the modern compiler 0.42.0 profile, not a replacement
for its type, ownership, origin or coverage rules. It omits legacy spellings and
parser recovery details. `name`, `integer` and `string_literal` are lexical tokens.
Quoted text denotes source tokens; `?` means optional, `*` repetition, `+` one or
more, and `|` alternatives. Parentheses group grammar expressions. A `list(x)` is
one or more comma-separated `x`; a `trailing_list(x)` additionally permits a final
comma. Empty lists are made optional at the use site.

## Files and declarations

```ebnf
file             = declaration+ ;
declaration      = import | public? (control | struct | variant | outcome | trait)
                 | implementation ;
public           = "pub" ;
import           = "import" string_literal "as" name ";" ;
qualified_name   = name ("." name)* ;
data_parameters  = "<" list(name) ">" ;
control_parameters = "<" list(name (":" bounds)?) ">" ;
bounds           = qualified_name ("+" qualified_name)* ;

struct           = "struct" name data_parameters? origin_groups?
                   "{" struct_field* "}" ";"? ;
origin_groups    = "[" list("'" name) "]" ;
struct_field     = public? type origin_mapping? name ("<-" "'" name)? ";" ;
variant          = "variant" name data_parameters? origin_groups?
                   "{" variant_alternatives "}" ";"? ;
variant_alternatives = trailing_list(name ("{" trailing_list(variant_field)? "}")?) ;
variant_field    = type origin_mapping? name ("<-" "'" name)? ;
outcome          = "outcome" name data_parameters? origin_groups?
                   "{" variant_alternatives "}" ";"? ;
alternatives     = trailing_list(name payload_declaration?) ;
payload_declaration = "{" trailing_list(field)? "}" ;
field            = type name ;

trait            = "trait" name "{" (associated_requirement | control_signature)* "}" ;
associated_requirement = "type" name (":" bounds)? ";" ;
control_signature = "control" name "(" trailing_list(field)? ")"
                    "exits" explicit_exits ";" ;
implementation   = "implement" qualified_name "for" type
                   "{" (associated_assignment | control)* "}" ;
associated_assignment = "type" name "=" type ";" ;
```

A struct field maps a grouped nested type before its name. A reference field uses
`<- 'group` after its name for storage; if its referent has origin groups, its
bracket mapping and storage arrow are both required. Variant payload fields use
the same origin mappings, as do named outcome payload fields. All layouts must be finite and
nonrecursive. Visibility applies to declarations and individual struct fields.
Implementations do not use `pub`; trait/type visibility determines their availability.

Trait associated bounds currently accept sealed `copy`/`clone`/`drop` capabilities;
user trait bounds and associated equality constraints are not implemented there.
Control type parameters may use user traits as bounds. Data parameters cannot have
bounds. Trait operations and implementations require explicit completion and origin
contracts; generic trait methods and blanket implementations are outside this profile.

## Types and origin annotations

```ebnf
type             = "ref" "mutable"? type | named_type ;
named_type       = qualified_name ("<" list(type_argument) ">" ("." name)*)? ;
type_argument    = type | integer ;
origin_mapping   = "[" list(origin) "]" ;
origin           = "'" name | input_origin ;
input_origin     = name ("[" "'" name "]")? ;
result_field     = type origin_mapping? name ("from" input_origin)? ;
```

Integer type arguments are used for array lengths, not general const generics.
`outcome<producer>` uses the checked producer identity in its angle brackets;
`outcome<identity<string>>` can name a specialized producer. Associated projections
have the form `reader<r>.item`. Syntactic nesting of `ref` does not imply references
to references are supported. Likewise, `auto` is restricted to supported inference
sites, and arbitrary reference type arguments are not generally permitted.

A result's `from input` and group mapping express dependencies; they are not runtime
arguments. `[input['a]]` maps a returned group to an externally carried group,
whereas `[input]` anchors it to input storage. Bare apostrophe group names are used
inside schema declarations. Their valid positions and privacy rules are checked
semantically; they are not interchangeable.

## Controls, bodies and paths

```ebnf
control          = "control" name control_parameters?
                   "(" trailing_list(field)? ")" "exits" exits body ;
exits            = "auto" | explicit_exits | inferred_exits ;
explicit_exits   = named_type origin_mapping? | list(exit_alternative)
                 | "{" trailing_list(exit_alternative) "}" ;
exit_alternative = name ("{" trailing_list(result_field)? "}")? ;
inferred_exits   = list(name "..." | exit_alternative) ;
body             = "{" body_item* "}" ;
body_item        = statement+ | selected | scoped | path_alias | tail_loop ;
selected         = "[" mask "]" ":" statement+ ;
scoped           = "[" mask "]" "over" "&" ":" body ;
path_alias       = "path" name "=" mask ";" ;
mask             = intersection ("|" intersection)* ;
intersection     = path_primary ("&" path_primary)* ;
path_primary     = name | "(" mask ")" ;
tail_loop        = "loop" name "(" list(field "=" expression)? ")" body ;
```

The general `statement+` body item is the implicit entry form. After an explicit
selection, following statements stay selected until another body item starts;
resuming the enclosing selection requires `[entry]:`. There is no arbitrary
unselected block statement. A tail loop must be last in its scope, and nested
source loops are not supported. Empty live fallthrough is not a completion.

A single bare exit name is resolved against named outcome declarations. If one
exists, it supplies the contract. `exits { ready }` or `exits ready {}` explicitly
means one nullary alternative. An ellipsis-marked name is always an individual
alternative, with a fixed nonempty inferred payload.

## Statements and deltas

```ebnf
statement        = binding | assignment | flat_delta | exit | forwarding
                 | call ";" | "emit" "(" expression ")" ";"
                 | "drop" "(" expression ")" ";"
                 | "again" name "(" trailing_list(expression)? ")" ";" ;
binding          = "mutable"? type name "=" initializer ";" ;
initializer      = expression | "capture" call | inline_delta ;
assignment       = expression "=" expression ";" ;
exit             = "exit" name ";"
                 | "exit" name "{" trailing_list(expression)? "}" ";"
                 | "exit" inline_delta ";" ;
forwarding       = "forward" call ";" ;
flat_delta       = (name "=")? "delta" expression
                   "{" trailing_list(flat_arm) "}" ";"? ;
flat_arm         = qualified_name payload_declaration? "=>" name ;
inline_delta     = "delta" expression "{" trailing_list(inline_arm) "}" ;
inline_arm       = qualified_name payload_declaration? "=>"
                   ("[" name "]" ":")? outcome_expression ;
outcome_expression = name "{" trailing_list(expression)? "}"
                   | inline_delta | "forward" ;
```

Only assignable places may be assignment targets. An ordinary call statement must
have one nullary completion. Flat arms bind path-dependent outer values; inline
arms introduce local scopes. `forward` as an arm result forwards that arm's input
payload. `forward call(...);` delegates the whole call. Bare `exit name;` forwards
a bound completed outcome or selects a nullary label under the main reference's
name-resolution rules. It cannot return an arbitrary ordinary value.

Direct brace construction as a delta operand needs parentheses:
`delta (some { 1 }) { ... }`. This distinguishes operand construction from the
arm block. Adapters are supported in tail exits and binding initializers, not
arbitrary expression, assignment or call-argument positions.

## Expressions

```ebnf
expression       = conjunction ("||" conjunction)* ;
conjunction      = comparison ("&&" comparison)* ;
comparison       = prefix (("==" | "!=" | "<" | "<=" | ">" | ">=") prefix)? ;
prefix           = ("!" | "borrow" "mutable"? | "deref") prefix | primary ;
primary          = atom ("." name)* ;
atom             = literal | qualified_name | "(" expression ")"
                 | call | construction | list_literal | "map" "{" "}" ;
call             = named_type "(" trailing_list(expression)? ")" ;
construction     = named_type "{" trailing_list(expression)? "}"
                 | named_type "{" trailing_list(name ":" expression) "}" ;
list_literal     = "[" trailing_list(expression)? "]" ;
literal          = integer | string_literal | "bytes" string_literal
                 | "true" | "false" | "unit" ;
```

`named_type` in call/construction positions describes a qualified spelling with
optional explicit arguments; semantic resolution determines whether it denotes a
callable, type or constructor. Named construction fields are supported only for
structs; variant/outcome payloads stay positional. An empty list or map needs an
expected type. `none` is a contextually typed constructor name, not a general null.
Prefix syntax still obeys ownership: reading Copy data through `deref` can be valid,
whereas moving an owned string out through it is rejected.

A named completion contract can bind its groups as `exits contract[input]`.
Options inherit their payload's ordered groups; a direct reference payload gives
the option a single builtin group named `'source`. These are dependency annotations,
not type arguments or runtime fields. `delta borrow value`, `delta borrow mutable value`
and `delta reference` inspect a sum without consuming it; see the
[borrowed payload typing rules](../language-reference.md#borrowing-whole-sums).
