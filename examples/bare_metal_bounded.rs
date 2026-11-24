use scarp::native_object::{
    emit_freestanding_function, FreestandingFunctionMode, FreestandingTarget,
};
use std::{env, error::Error, fs::OpenOptions, io::Write};

fn main() -> Result<(), Box<dyn Error>> {
    let source = include_str!("../experiments/bare-metal/bounded_sum.srp");
    let artifact = emit_freestanding_function(
        source,
        FreestandingTarget::X86_64None,
        FreestandingFunctionMode::Bounded { max_work: 1000 },
    )?;
    let kernel = artifact.kernel;
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
