//! Run the compiler-owned input fixture with no process-global I/O.
use std::io::Cursor;

fn main() -> Result<(), scarp::RunError> {
    let source = include_str!("program_input.srp");
    let mut input = Cursor::new(include_bytes!("program_input.stdin"));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    scarp::run_with_input(source, &mut input, &mut out, &mut err)?;
    assert_eq!(out, include_bytes!("program_input.stdout"));
    assert!(err.is_empty());
    Ok(())
}
