use scarp::native_object::emit_elf64_bounded_integer_function;
use std::{env, error::Error, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let source = include_str!("../experiments/bare-metal/bounded_sum.srp");
    let kernel = emit_elf64_bounded_integer_function(source, 1000)?;
    assert!(kernel.object.starts_with(b"\x7fELF"));
    assert!(!kernel.failure_spans.is_empty());
    if let Some(path) = env::args_os().nth(1) {
        let mut object = OpenOptions::new().write(true).create_new(true).open(path)?;
        object.write_all(&kernel.object)?;
    }
    println!(
        "scarp_bounded_integer_function: {} ELF64 bytes",
        kernel.object.len()
    );
    Ok(())
}
