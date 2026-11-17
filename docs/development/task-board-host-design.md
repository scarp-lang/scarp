# Host-driven task board UI design

Status: proposed. The [alpha.12 task board](../task-board.md) is an
interpreter-backed console project. Its pure `apply(Board, Action)` transition
and immutable snapshots provide the domain logic for a future hosted UI.
The current Rust execution APIs run an entry program. A host-call API for a
named typed Scarp function is a separate compiler milestone.

## First host and state model

Start with one Linux desktop window in a future, explicitly selected host.
Use a retained view model because the task list, edit fields and focus have
stable identities across event deliveries. The host renders and owns windows,
widgets, editable text buffers, focus and the accessibility tree. Scarp owns
validation and task transitions. The initial widget set is a window, heading,
labeled text input, button, status text and a bounded list of rows. List rows
have stable host IDs associated with task titles. Keyboard traversal and
activation must reach every control. Status updates expose the relevant error
or success text to the host accessibility bridge.

| Host event | Checked action or host operation |
| --- | --- |
| Submit task title | Validate the UTF-8 length, construct the declared add action and pass it to the reducer |
| Complete selected task | Map the selected row to its title and pass the declared complete action |
| Change focus or edit an input buffer | Update transient host view state |
| Close the window | Stop accepting events and drop renderer and program resources |

The host serializes events on one UI thread and associates each dispatch
with the current state generation. On a successful transition, replace
the state snapshot and schedule rendering. On a domain error, retain the
prior state and show the returned message. On a runtime diagnostic or
budget failure, retain the snapshot, report the diagnostic and release
event-local values. Discard late completions after a newer dispatch or
window close. The initial view re-renders the bounded list and preserves
focus by stable control ID.

## Compiler boundary

A future embedding interface resolves a public function in a compiled
module, verifies purity and its full parameter/result signature in that
program's type registry, and invokes the existing interpreter with explicit
work, live-slot and payload limits. Resolve nominal types against the same
program that constructed the values. Own the immutable result in the host
until rendering completes. Each event either commits the whole next board
or retains the earlier board. Limit the event queue, input bytes, work
per dispatch and retained snapshots. Keep filesystem, network, clock,
process and window access in separately granted host capabilities. Static
verification and editor queries receive no runtime capabilities.

## Milestones

- **U0** Review the event and API contract. Re-run the published task board
  and its lifecycle, validation, and capacity tests as the domain proof.
- **U1** Implement the narrow typed function-call API and a headless CLI or
  Rust test harness. Cover wrong nominal identity, impure functions,
  invalid actions, exhausted budgets, and render-state rollback.
- **U2** Build one interactive Linux host with keyboard traversal,
  meaningful accessible labels, input dispatch and close cleanup.
- **U3** Validate layout, focus restoration, accessibility events and
  multi-window lifecycle.
- **U4** Verify packaging, startup and rendering on a clean machine.
- **U5** Introduce additional platforms after independent host validation.
