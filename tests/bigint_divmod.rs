use scarp::{
    ast::ExpressionKind,
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    modules::{self, MemorySourceLoader, ModuleLimits},
    types::{FunctionEffect, PrimitiveType, TypeKind},
    ExecutionLimits, ExecutionOptions, RunError,
};

fn run(source: &str) -> String {
    let mut stdout = Vec::new();
    scarp::run(source, &mut stdout).expect("valid Scarp source");
    String::from_utf8(stdout).expect("UTF-8 output")
}

fn fails_atomically(source: &str, options: ExecutionOptions, expected: Kind) -> Diagnostic {
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let RunError::Diagnostic(error) =
        scarp::run_with_io_and_options(source, &mut stdout, &mut stderr, options)
            .expect_err("expected resource/type diagnostic")
    else {
        panic!("expected typed diagnostic")
    };
    assert_eq!(error.kind, expected);
    assert!(
        stdout.is_empty() && stderr.is_empty(),
        "outputs remain atomic"
    );
    assert_eq!(
        scarp::check_with_options(source, options).unwrap_err(),
        error
    );
    error
}

#[test]
fn divmod_has_exact_structural_type_and_truncated_signed_semantics() {
    let source = r#"
        bind a: tuple<bigint,bigint> = bigint_divmod(bigint("-7"),bigint("3"));
        bind b = bigint_divmod(bigint("7"),bigint("-3"));
        bind c = bigint_divmod(bigint("-7"),bigint("-3"));
        print(a,a[0],a[1],b[0],b[1],c[0],c[1]);
        print(bigint_divmod(bigint("0"),bigint("3")));
    "#;
    assert!(scarp::compile(source).is_ok());
    assert_eq!(run(source), "(-2, -1) -2 -1 -2 1 2 -1\n(0, 0)\n");
}

#[test]
fn divmod_handles_signed_i64_boundaries_and_exact_large_oracles() {
    assert_eq!(
        run("print(bigint_divmod(bigint(\"-9223372036854775808\"),bigint(\"-1\")));"),
        "(9223372036854775808, 0)\n",
    );
    let source = r#"
       bind a=bigint("12345678901234567890123456789012345678901234567890");
       bind b=bigint("98765432109876543210987654321");
       bind qr=bigint_divmod(a,b);
       print(qr[0],qr[1],qr[0]*b+qr[1]==a);
       bind c=bigint("-987654321098765432109876543210987654321");
       bind d=bigint("12345678901234567890");
       bind z=bigint_divmod(c,d);
       print(z[0],z[1],z[0]*d+z[1]==c);
    "#;
    // These exact signed quotients and remainders were computed with a
    // separately implemented Python integer truncation oracle.
    assert_eq!(
        run(source),
        "124999998860937500014 23533950614699073961469907396 true\n-80000000729000006634 -8642720610864272061 true\n",
    );
    assert!(scarp::check(source).is_ok());
}

#[test]
fn divmod_supports_typed_pure_functions_and_nested_values() {
    let source = r#"
       pure funcra split(x:bigint,y:bigint)->tuple<bigint,bigint> {
           return bigint_divmod(x,y);
       }
       bind pair=split(bigint("123"),bigint("10"));
       bind nested=(pair,"exact");
       print(pair,nested[0][0],nested[0][1],nested[1]);
    "#;
    assert_eq!(run(source), "(12, 3) 12 3 exact\n");
}

#[test]
fn divmod_refuses_wrong_types_or_arity_at_compile_time() {
    for expression in [
        "bigint_divmod(bigint(\"7\"))",
        "bigint_divmod(bigint(\"7\"),bigint(\"3\"),bigint(\"2\"))",
        "bigint_divmod(7,bigint(\"3\"))",
        "bigint_divmod(bigint(\"7\"),3)",
        "bigint_divmod(bigint(\"7\"),3.0)",
        "bigint_divmod(left:bigint(\"7\"),right:bigint(\"3\"))",
    ] {
        assert!(
            scarp::compile(&format!("print({expression});")).is_err(),
            "{expression}"
        );
    }
    assert!(scarp::compile(
        "bind x:tuple<bigint,integer>=bigint_divmod(bigint(\"7\"),bigint(\"3\"));"
    )
    .is_err());
}

#[test]
fn divmod_is_atomic_for_division_zero_and_resource_exhaustion() {
    let zero =
        r#"print("pending"); eprint("pending"); print(bigint_divmod(bigint("7"),bigint("0")));"#;
    let error = fails_atomically(zero, ExecutionOptions::default(), Kind::DivisionByZero);
    assert_eq!(&zero[error.span.start..error.span.end], r#"bigint("0")"#);
    let source = "print(\"pending\"); print(bigint_divmod(bigint(\"7777777\"),bigint(\"7\")));";
    fails_atomically(
        source,
        ExecutionOptions {
            max_tuple_bytes: 0,
            ..ExecutionOptions::default()
        },
        Kind::TupleAllocation,
    );
    fails_atomically(
        source,
        ExecutionOptions {
            max_bigint_digits: 6,
            ..ExecutionOptions::default()
        },
        Kind::BigIntLimit,
    );
    let operands_only = ExecutionOptions {
        max_bigint_bytes: 8,
        ..ExecutionOptions::default()
    };
    scarp::check_with_options(r#"print(bigint("7777777"),bigint("7"));"#, operands_only).unwrap();
    let error = fails_atomically(source, operands_only, Kind::BigIntLimit);
    assert_eq!(&source[error.span.start..error.span.end], r#"bigint("7")"#);
    fails_atomically(
        source,
        ExecutionOptions {
            limits: ExecutionLimits {
                max_steps: 8,
                ..ExecutionLimits::default()
            },
            ..ExecutionOptions::default()
        },
        Kind::ExecutionLimit,
    );
}

#[test]
fn typed_divmod_is_available_under_the_main_compiler_strict_tuple_model() {
    let source = "bind pair:tuple<bigint,bigint>=bigint_divmod(bigint(\"256\"),bigint(\"16\")); print(pair[0]==bigint(\"16\"),pair[1]==bigint(\"0\"));";
    let program = scarp::compile(source).unwrap();
    let (kind, signature) = program
        .typed_expressions()
        .find_map(|expr| {
            expr.nodes()
                .find(|node| matches!(node.syntax, ExpressionKind::UserCall { name, .. } if name == "bigint_divmod"))
                .map(|node| (node.kind, node.call_signature.unwrap()))
        })
        .unwrap();
    let bigint = PrimitiveType::BigInt.id();
    assert!(
        matches!(program.type_registry().get(kind), Some(TypeKind::Tuple(elements))
            if elements.as_ref() == [bigint, bigint])
    );
    assert!(
        matches!(program.type_registry().get(signature), Some(TypeKind::Function { parameters, result, effect: FunctionEffect::Pure })
            if parameters.as_ref() == [bigint, bigint] && *result == kind)
    );
    assert_eq!(run(source), "true true\n");
}

#[test]
fn imported_divmod_preserves_tuple_type_and_conditional_evaluation() {
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            r#"
                import "lib.srp" as lib;
                bind a=bigint("-7");
                print(if true { lib.split(a,bigint("\u{33}")) }
                    else { bigint_divmod(a,bigint("0")) });
            "#,
        )
        .unwrap();
    loader
        .insert(
            "lib.srp",
            "pub pure funcra split(a:bigint,b:bigint)->tuple<bigint,bigint> {return bigint_divmod(a,b);}",
        )
        .unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    program.check(ExecutionOptions::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"(-2, -1)\n");
    assert!(err.is_empty());
}
