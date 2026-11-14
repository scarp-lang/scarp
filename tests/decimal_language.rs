//! End-to-end exact Decimal tests for the interpreter development branch.
//! Decimal input is explicit text; binary64, integer and bigint never coerce.

use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{self, MemorySourceLoader, ModuleLimits},
    types::{PrimitiveType, TypeKind},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> String {
    let mut out = Vec::new();
    scarp::run(source, &mut out).expect("valid exact Scarp decimal program");
    String::from_utf8(out).unwrap()
}

fn fails_atomically(source: &str, kind: Kind) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let failure =
        scarp::run_with_io_and_options(source, &mut out, &mut err, ExecutionOptions::default())
            .unwrap_err();
    let RunError::Diagnostic(problem) = failure else {
        panic!("expected a typed decimal diagnostic");
    };
    assert_eq!(problem.kind, kind, "{source}");
    assert!(
        out.is_empty() && err.is_empty(),
        "failed runs never replay prints"
    );
}

#[test]
fn exact_decimal_arithmetic_uses_base_ten_not_binary64() {
    let source = r#"
        bind a: decimal = decimal("0.1");
        bind b = decimal("0.2");
        print(a + b, a * b, b - a, -a);
        print(decimal("1.20") == decimal("1.2"),
              decimal("-10") < decimal("-9.99"));
        print(decimal("1") / decimal("8"), decimal_abs(decimal("-2.50")));
        print(decimal_from_integer(42), decimal_to_integer(decimal("42.0")));
        print(decimal_to_text(decimal("1.200")));
        print(0.1 + 0.2);
    "#;
    assert_eq!(
        run(source),
        "0.3 0.02 0.1 -0.1\ntrue true\n0.125 2.5\n42 42\n1.2\n0.30000000000000004\n"
    );
}

#[test]
fn explicit_rounding_is_deterministic_and_preserves_display_places() {
    assert_eq!(
        run(r#"
            print(decimal_div_rounded(decimal("1"), decimal("3"), 2, "half_even"));
            print(decimal_div_rounded(decimal("1"), decimal("8"), 2, "half_up"));
            print(decimal_round(decimal("1.225"), 2, "half_even"));
            print(decimal_round(decimal("-1.225"), 2, "half_up"));
            print(decimal_format(decimal("1.2"), 2, "half_even"));
            print(decimal_format(decimal("-0.005"), 2, "half_even"));
        "#),
        "0.33\n0.13\n1.22\n-1.23\n1.20\n0.00\n"
    );
}

#[test]
fn decimal_is_a_real_typed_value_in_functions_arrays_tuples_and_signatures() {
    let source = r#"
        pure funcra total(price: decimal, tax: decimal) -> decimal {
            return price + price * tax;
        }
        bind cost = total(decimal("199.99"), decimal("0.18"));
        bind readings: array<decimal, 2> = [array: cost, decimal("3.00")];
        bind grouped: tuple<decimal, string> = (readings[0], "exact");
        print(decimal_format(grouped[0], 2, "half_even"), grouped[1]);
        var copy = readings[1];
        copy += decimal("0.1");
        print(copy, readings[1]);
    "#;
    let compiled = scarp::compile(source).unwrap();
    let decimal = PrimitiveType::Decimal.id();
    assert!(compiled
        .type_registry()
        .iter()
        .any(|(id, kind)| id == decimal
            && matches!(kind, TypeKind::Primitive(PrimitiveType::Decimal))));
    assert_eq!(run(source), "235.99 exact\n3.1 3\n");
}

#[test]
fn bad_decimal_types_are_rejected_at_compile_time_without_coercion() {
    for expression in [
        r#"decimal("1") + 1"#,
        r#"decimal("1") + 1.0"#,
        r#"decimal("1") + bigint("1")"#,
        r#"decimal("1") == 1"#,
        r#"decimal("1") % decimal("2")"#,
        r#"decimal("1") & decimal("2")"#,
        r#"decimal(1)"#,
        r#"decimal_from_integer(1.0)"#,
        r#"decimal_round(decimal("1"), "2", "half_even")"#,
        r#"decimal_div_rounded(decimal("1"), decimal("2"), 2)"#,
        r#"decimal_format(decimal("1"), 2, 3)"#,
    ] {
        let src = format!("print({expression});");
        assert!(scarp::compile(&src).is_err(), "{expression}");
    }
    assert!(scarp::compile(r#"bind x: float = decimal("1.1");"#).is_err());
    assert!(scarp::compile(r#"bind x: decimal = 1.1;"#).is_err());
}

#[test]
fn runtime_errors_are_typed_and_do_not_commit_partial_output() {
    for (expr, kind) in [
        (r#"decimal("nope")"#, Kind::InvalidDecimal),
        (r#"decimal("0.0000000000000000001")"#, Kind::DecimalLimit),
        (
            r#"decimal("99999999999999999999999999999")"#,
            Kind::DecimalLimit,
        ),
        (r#"decimal("1") / decimal("3")"#, Kind::DecimalInexact),
        (r#"decimal("1") / decimal("0")"#, Kind::DivisionByZero),
        (
            r#"decimal_round(decimal("1"), 19, "half_even")"#,
            Kind::DecimalLimit,
        ),
        (
            r#"decimal_round(decimal("1"), -1, "half_even")"#,
            Kind::DecimalLimit,
        ),
        (
            r#"decimal_round(decimal("1"), 2, "unsupported")"#,
            Kind::InvalidDecimal,
        ),
        (
            r#"decimal("9999999999999999999999999999") * decimal("2")"#,
            Kind::DecimalLimit,
        ),
        (
            r#"decimal_to_integer(decimal("1.5"))"#,
            Kind::DecimalInexact,
        ),
    ] {
        let src = format!("print(\"pending\"); eprint(\"pending\"); print({expr});");
        fails_atomically(&src, kind);
    }
}

#[test]
fn modules_preserve_exact_decimal_types_and_runtime_failure_locations() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "lib.srp",
            r#"
            pub pure funcra calculate(net: decimal) -> decimal {
                return net + decimal("0.25");
            }
        "#,
        )
        .unwrap();
    loader
        .insert(
            "main.srp",
            r#"
            import "lib.srp" as lib;
            print(lib.calculate(decimal("1.15")));
        "#,
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    program.check(ExecutionOptions::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"1.4\n");
    assert!(err.is_empty());
}

#[test]
fn decimal_example_fixture_matches_compiler_output() {
    let source = include_str!("../examples/decimal_exact.srp");
    let expected = include_bytes!("../examples/decimal_exact.stdout");
    assert_eq!(run(source).as_bytes(), expected);
}

#[test]
fn experimental_native_backends_reject_exact_decimal_instead_of_coercing() {
    let source = r#"bind price: decimal = decimal("1.20"); print(price);"#;
    assert_eq!(
        scarp::c_backend::emit_c(source).unwrap_err().kind,
        Kind::NativeUnsupported
    );
    assert_eq!(
        scarp::native_object::emit_elf64_integer_kernel(source)
            .unwrap_err()
            .kind,
        Kind::NativeUnsupported
    );
}
