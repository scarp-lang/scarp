# Source records and application state

Declare an immutable record at file scope with `record Task { title: string, done: boolean }`.
Use `record Task` wherever a type is required, including in `tuple<record Task, boolean>` and
`array<record Task, 2>`. A declaration can refer to another record declared later in the same
file. The compiler checks every declaration, including any unused layout.

Construct a value with `new Task { title: "Draft", done: false }`. Name every field exactly
once. Initializers run in written order. Field display follows declaration order.
Use `task->title` to read a field. Projections chain with indexing, for example
`panel->items[0]->title`. A `var` can receive a replacement record of the same declared type.
Existing copies retain their original values. Equality compares the declared type and contents.

Export a declaration with `pub record`. With `import "tasks.srp" as tasks;`, refer to the type as
`record tasks.Task` and construct it with `new tasks.Task { ... }`. Public record fields and
public function signatures require public record types. Module-level constants are scalar.

Record types are nominal and scoped to the compiled program. Layouts have up to 32 fields
and share the existing composite depth, type-node and expanded-value bounds. Recursive layouts
are rejected. Construction draws from the cumulative tuple and record budget
(`--max-tuple-bytes`). Record display and equality participate in the execution budget.
The scalar C11 and ELF emitters diagnose record operations as unsupported.

Run the task-panel model with `scarp exec examples/app_state.srp`. Its pure transition checks
a title, returns a state and message tuple, and uses destructuring to read the result. The model
changes the first entry of a fixed array and preserves a previously captured state.
