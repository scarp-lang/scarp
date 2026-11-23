//! Demonstrate Rust-only named callable invocation:
 //! cargo run --example host_callable_invocation

use scarp::{resolver::CallableCatalog, value::Value, ExecutionOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let program = scarp::compile(
        "pure funcra triple(n: integer) -> integer { return n * 3; }",
    )?;
    let handle = CallableCatalog::from_program(program).bind_exact(0)?;
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = handle.invoke_with_io_and_options(
        vec![Value::Integer(7)],
        &mut out,
        &mut err,
        ExecutionOptions::default(),
    )?;
    assert_eq!(result, Value::Integer(21));
    assert!(out.is_empty() && err.is_empty());
    println!("{result}");
    Ok(())
}
