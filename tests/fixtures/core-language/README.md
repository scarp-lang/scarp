# Core language conformance fixtures

These are compiler-owned executable regressions, not public teaching content.
They run through the real module compiler and explicit-reader interpreter in
`cargo test --locked --offline --test core_language_conformance`.
The input fixture needs the input development implementation (PR #181); it does
not run on published v0.0.6-alpha.5. The remaining supported positive cases use
existing published primitives and syntax.

| Fixture | Behavior locked by the test |
| --- | --- |
| hello | Exact Hello World line |
| bindings | Immutable/mutable variables, update and lexical shadowing |
| integers | Checked integer arithmetic, quotient/remainder, i64 minimum |
| floats | Finite arithmetic and explicit exact scalar conversions |
| booleans | Comparisons, logic and short-circuit division avoidance |
| strings | Unicode concatenation, scalar indexing and text queries |
| user_input | Two lines, raw CRLF and a final unterminated line |
| conditionals | if/elif/else selection |
| loops | repeat/while, break, continue, updates and scopes |
| functions | Typed parameters, named/default arguments, return and unit |
| recursion | Recursive function with pre/postconditions |
| imports | Public module constant/function and typed signature |
| type_error | Type mismatch before execution |
| invalid_statement | Malformed output syntax |
| input_eof | EOF distinguished from an empty line; atomic streams |
| runtime_failure | Selected division error; atomic streams |
| resource_limit | Infinite-loop budget exhaustion; atomic streams |
| pure_input | Compile-time purity rejection |

Every positive `.srp` has an exact `.stdout`. Optional `.stdin` files supply raw
bytes; `.gitattributes` disables line-ending conversion for these inputs. Error
cases have a canonical `.error` code, and resource_limit has a `.steps` override.
The shared harness checks source-aware diagnostics and empty stdout/stderr on
failure. `support/math.srp` is an imported library, not an entry fixture.

The more detailed input, output, numeric, function and module suites retain
boundary, malformed-source and regression coverage. This suite checks their
ordinary combined use and does not redefine the language or add new syntax.

See the [compiler source-word inventory](../../../docs/development/core-language-keyword-inventory.md)
for the exact keyword/context distinction and intentionally missing constructs.
