use scarp::native_object::emit_elf64_bounded_integer_entry_with_layout;
use std::{env, error::Error, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let source = include_str!("../experiments/bare-metal/direct_calls.srp");
    let stack_budget = 512;
    let artifact =
        emit_elf64_bounded_integer_entry_with_layout(source, "evaluate", 1000, stack_budget)?;
    assert_eq!(artifact.max_active_frames, 3);
    if let Some(path) = env::args_os().nth(1) {
        let mut object = OpenOptions::new().write(true).create_new(true).open(path)?;
        object.write_all(&artifact.kernel.object)?;
    }
    println!(
        "scarp_bounded_integer_entry: {} ELF64 bytes, {} of {stack_budget} stack bytes",
        artifact.kernel.object.len(),
        artifact.peak_stack_bytes,
    );
    Ok(())
}
