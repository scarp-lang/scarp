# Core source-word inventory

Audited against published v0.0.6-alpha.6, source
`cdf6a3924ec2a36c7beca5a9aec4f01a0bce515e`. This compiler-owned inventory records parser and
resolver behavior; it does not introduce new syntax. The lexer emits these
words as identifier tokens and the parser recognizes them by context.

| Words | Implemented parsing/semantic role | Name policy |
| --- | --- | --- |
| `bind`, `var` | Required immutable/mutable initialization | Reserved |
| `print`, `println`, `write`, `eprint`, `ewrite` | Output statements, with newline/stream semantics | Reserved; not expression functions |
| `if`, `elif`, `else` | Conditional statement and successive branches | Reserved |
| `while` | Boolean conditional loop | Reserved |
| `break`, `continue` | Nearest-loop control | Reserved |
| `funcra`, `pure` | Typed function declaration and effect restriction | Reserved |
| `return` | Function return with declared result or unit | Reserved |
| `needs`, `ensures` | Pure pre/postconditions | Reserved |
| `true`, `false` | Boolean literal values | Reserved |
| `and`, `or`, `not` | Short-circuit logical operators and negation | Reserved |
| `repeat`, `as` | Counted-repeat statement and loop index introduction | Contextual; ordinary binding names remain valid; `repeat` cannot name a function |
| `import`, `as`, `pub` | Source import alias and exported declaration | Contextual module syntax; resolution enforces position/visibility |
| `integer`, `float`, `boolean`, `string`, `vector`, `integers`, `tile`, `unit` | Existing primitive type annotations/signatures | Contextual type names; unit is a function-result marker, not a general stored value |
| `tuple` | Structural `tuple<...>` type annotations/signatures | Contextual type constructor; ordinary binding/function names remain valid |
| `let`, `mut` | Recognized legacy spelling for migration diagnostics | Reserved and rejected as current declarations |

`read_line` uses ordinary call syntax and is an impure intrinsic in
published v0.0.6-alpha.6. It is absent from alpha.5 and is not a
new reserved keyword. User-defined functions retain call-name precedence, as
with the existing numerical/text built-ins. Input never enters the pure numeric
registry. `ask`, `for` and `match` have no built-in execution semantics in this
slice; seeing those ordinary identifiers does not implement the future feature.

The authoritative sites are `src/parser.rs` (statement/expression/module/type
contexts), `src/resolver.rs::check_name` (reserved names), function declaration
validation (`repeat`) and `src/types.rs::PrimitiveType` / `src/types/source.rs::SourceType`
(primitive identities and structural source types).
`tests/repeat.rs`, `tests/binding_syntax.rs`, `tests/output.rs`,
`tests/functions.rs` and `tests/core_language_conformance.rs` exercise the rules.
The [tuple contract](../tuples.md) defines implemented composite syntax. The
type-staging proposal and proposal 0036 govern future composites; this inventory
does not reserve speculative words.
