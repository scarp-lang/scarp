# Calculator project

From this directory, run `scarp verify` to check the application, then `scarp run`
to print `5`. Run `scarp project info --json` to see the manifest metadata.

```sh
scarp test --list
scarp test --require-tests
scarp test --json --filter input
```

The application and both tests import the same `add` function. `input.srp`
reads two lines from its sibling `input.stdin` fixture. Change `25` to `24`
in a local copy of that fixture to see an `E0065` assertion failure.

To start a project of your own, run `scarp init my-calculator`, then copy the
two source files and the contents of `tests/` into the generated directories.
