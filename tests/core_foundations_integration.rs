use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, ModuleRunError},
    ExecutionOptions, InputLimits,
};
use std::{io::Cursor, path::Path};

fn program() -> modules::ModuleProgram {
    modules::compile_file(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/core_input_tuple.srp"),
        Default::default(),
    )
    .unwrap_or_else(|e| panic!("{}", e.render()))
}

#[test]
fn input_text_and_imported_tuple_values_compose_and_reuse_independent_readers() {
    let program = program();
    assert_eq!(program.inspection_schema_version(), 2);
    for _ in 0..2 {
        let mut input = Cursor::new(include_bytes!("../examples/core_input_tuple.stdin"));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        program
            .execute_with_input(
                &mut input,
                &mut out,
                &mut err,
                Default::default(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(out, include_bytes!("../examples/core_input_tuple.stdout"));
        assert!(err.is_empty());
        assert_eq!(input.position(), input.get_ref().len() as u64);
    }
}

#[test]
fn tuple_and_input_limits_remain_independent_and_fail_without_output_replay() {
    let program = program();
    for (options, limits, kind) in [
        (
            ExecutionOptions {
                max_tuple_bytes: 0,
                ..Default::default()
            },
            InputLimits::default(),
            Kind::TupleAllocation,
        ),
        (
            ExecutionOptions::default(),
            InputLimits {
                max_reads: 1,
                ..Default::default()
            },
            Kind::InputLimit,
        ),
        (
            ExecutionOptions {
                max_text_bytes: 0,
                ..Default::default()
            },
            InputLimits::default(),
            Kind::TextConstruction,
        ),
    ] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let error = program
            .execute_with_input(
                &mut Cursor::new(include_bytes!("../examples/core_input_tuple.stdin")),
                &mut out,
                &mut err,
                options,
                limits,
            )
            .unwrap_err();
        let ModuleRunError::Diagnostic(error) = error else {
            panic!("expected diagnostic")
        };
        assert_eq!(error.diagnostic.kind, kind, "{}", error.render());
        assert!(out.is_empty() && err.is_empty());
    }
}

#[test]
fn failed_conversion_preserves_imported_source_and_stops_before_the_next_read() {
    let mut input = Cursor::new(b"invalid\nnot consumed\n");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let error = program()
        .execute_with_input(
            &mut input,
            &mut out,
            &mut err,
            Default::default(),
            Default::default(),
        )
        .unwrap_err();
    let ModuleRunError::Diagnostic(error) = error else {
        panic!("expected diagnostic")
    };
    assert_eq!(error.diagnostic.kind, Kind::NumericConversion);
    assert!(
        error.render().contains("input_record.srp:2:"),
        "{}",
        error.render()
    );
    assert_eq!(input.position(), 8);
    assert!(out.is_empty() && err.is_empty());
}
