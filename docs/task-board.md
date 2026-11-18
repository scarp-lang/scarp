# Task board state example

The task board demonstrates structured application state in the alpha.12
interpreter. From the compiler repository or a native package, run:

```sh
scarp verify examples/projects/task-board
scarp launch examples/projects/task-board
scarp test --json --require-tests examples/projects/task-board
```

The project uses `scarp.project` and the public declarations in
`examples/projects/task-board/src/board.srp`. A `Board` stores tasks in
insertion order in a typed list. A map associates exact titles with list
indices. The pure `apply` function accepts an `Action` and returns a new
`Board` or a validation error. The pure `find` function returns
`option<record Task>`. A successful transition preserves the earlier
snapshot. Completing an already completed task returns the same state.

The project tests check UTF-8 titles, prior snapshots, missing or duplicate
titles, missing tasks, repeat completion and the 128-task limit. Create boards
with `empty()` and update them through `apply` so the list and index agree.

A future UI host can translate an event into an `Action`, apply it to the
current snapshot and render the returned state. Persistence would need
a versioned representation of the task list and validation on import, with
the index rebuilt from the list. The example runs as a CLI project. Host
event bindings and serialization remain outside its current scope.
