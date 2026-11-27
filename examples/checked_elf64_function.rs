use scarp::native_object::emit_elf64_checked_integer_function;
use std::{env, error::Error, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let source = include_str!("checked_elf64_function.srp");
    let (definition, _) = source.split_once("\nprint(").expect("example has a caller");
    let kernel = emit_elf64_checked_integer_function(definition)?;
    assert!(kernel.object.starts_with(b"\x7fELF"));
    assert_eq!(kernel.failure_spans.len(), 1);
    let span = kernel.failure_spans[0];
    assert_eq!(&definition[span.start..span.end], "*");
    if let Some(path) = env::args_os().nth(1) {
        let mut object = OpenOptions::new().write(true).create_new(true).open(path)?;
        object.write_all(&kernel.object)?;
    }
    println!(
        "scarp_checked_integer_function: {} ELF64 bytes",
        kernel.object.len()
    );
    Ok(())
}
