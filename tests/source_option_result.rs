use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind},
    types::{TypeKind, TypeLimits},
};

fn run(source: &str) -> String {
    let mut output = Vec::new();
    scarp::run(source, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

fn rejection(source: &str, kind: DiagnosticKind, at: &str) {
    let diagnostic: Diagnostic = scarp::compile(source).unwrap_err();
    assert_eq!(diagnostic.kind, kind, "{diagnostic:?}");
    assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], at);
}

#[test]
fn typed_submission_golden_and_exhaustive_extraction() {
    assert_eq!(
        run(include_str!("../examples/typed_submission.srp")).as_bytes(),
        include_bytes!("../examples/typed_submission.stdout")
    );
}

#[test]
fn concrete_instantiations_are_distinct_and_reuse_the_variant_registry() {
    let source = r#"
        bind a: option<integer> = new option<integer>::Some(4);
        bind b: option<integer> = new option<integer>::None();
        bind c: option<string> = new option<string>::Some("text");
        bind d: result<integer, string> = new result<integer, string>::Ok(7);
        bind e: result<integer, boolean> = new result<integer, boolean>::Err(true);
        match on a { case None {} case Some(value) { print(value); } }
        match on b { case None { print("none"); } case Some(_) {} }
        match on c { case None {} case Some(text) { print(text); } }
        match on d { case Ok(value) { print(value); } case Err(_) {} }
        match on e { case Ok(_) {} case Err(reason) { print(reason); } }
    "#;
    let program = scarp::compile(source).unwrap();
    let types = program.binding_types();
    let a = types[0].as_variant().unwrap();
    let b = types[1].as_variant().unwrap();
    let c = types[2].as_variant().unwrap();
    let d = types[3].as_variant().unwrap();
    let e = types[4].as_variant().unwrap();
    assert_eq!(a.type_id(), b.type_id());
    assert_ne!(a.type_id(), c.type_id());
    assert_ne!(d.type_id(), e.type_id());
    assert_eq!(a.cases()[0].0, "None");
    assert_eq!(a.cases()[1].0, "Some");
    assert_eq!(d.cases()[0].0, "Ok");
    assert_eq!(d.cases()[1].0, "Err");
    assert!(matches!(
        program.type_registry().get(a.type_id()),
        Some(TypeKind::Applied { constructor, arguments })
            if constructor.module == u32::MAX
                && constructor.declaration == 0
                && arguments.len() == 1
    ));
    assert!(matches!(
        program.type_registry().get(d.type_id()),
        Some(TypeKind::Applied { constructor, arguments })
            if constructor.module == u32::MAX
                && constructor.declaration == 1
                && arguments.len() == 2
    ));
    assert_eq!(run(source), "4\nnone\ntext\n7\ntrue\n");
}

#[test]
fn nested_specializations_preserve_checked_payload_types() {
    let source = r#"
        bind wrapped: option<result<integer, string>> =
            new option<result<integer, string>>::Some(
                new result<integer, string>::Ok(12)
            );
        match on wrapped {
            case None { print("empty"); }
            case Some(inner) {
                match on inner {
                    case Ok(number) { print(number); }
                    case Err(_) { print("error"); }
                }
            }
        }
    "#;
    assert_eq!(run(source), "12\n");
}

#[test]
fn constructor_cases_validate_missing_excess_and_wrong_payloads() {
    for (source, kind, at) in [
        (
            "bind x = new option<integer>::Some(\"bad\");",
            DiagnosticKind::TypeMismatch,
            "\"bad\"",
        ),
        (
            "bind x = new option<integer>::None(4);",
            DiagnosticKind::VariantCase,
            "None",
        ),
        (
            "bind x = new result<integer, string>::Err();",
            DiagnosticKind::VariantCase,
            "Err",
        ),
        (
            "bind x = new result<integer, string>::Ok(true);",
            DiagnosticKind::TypeMismatch,
            "true",
        ),
        (
            "bind x = new option<integer>::Unknown();",
            DiagnosticKind::VariantCase,
            "Unknown",
        ),
    ] {
        rejection(source, kind, at);
    }
}

#[test]
fn family_parameters_and_match_coverage_are_checked() {
    let mismatch = "bind value: option<string> = new option<integer>::Some(2);";
    rejection(
        mismatch,
        DiagnosticKind::TypeMismatch,
        "new option<integer>::Some(2)",
    );
    let unmatched = "match on new option<integer>::None() { case None {} }";
    let diagnostic = scarp::compile(unmatched).unwrap_err();
    assert_eq!(diagnostic.kind, DiagnosticKind::MatchCoverage);
    assert!(diagnostic.message.contains("Some"));
    let unmatched = "match on new result<integer, string>::Ok(2) { case Ok(_) {} }";
    let diagnostic = scarp::compile(unmatched).unwrap_err();
    assert_eq!(diagnostic.kind, DiagnosticKind::MatchCoverage);
    assert!(diagnostic.message.contains("Err"));
}

#[test]
fn unrelated_source_variants_remain_ordinary_nominal_types() {
    let source = r#"
        variant option { Legacy(integer) }
        bind old = new option::Legacy(3);
        bind fresh = new option<integer>::Some(4);
        match on old { case Legacy(x) { print(x); } }
        match on fresh { case None {} case Some(x) { print(x); } }
    "#;
    assert_eq!(run(source), "3\n4\n");
}

#[test]
fn concrete_wrappers_preserve_nominal_type_resource_gates() {
    for source in [
        "bind x = new option<integer>::Some(1);",
        "bind x = new result<integer, string>::Ok(1);",
    ] {
        let narrow_arity = TypeLimits {
            max_arity: 1,
            ..TypeLimits::default()
        };
        let diagnostic = scarp::compile_with_type_limits(source, narrow_arity).unwrap_err();
        assert_eq!(diagnostic.kind, DiagnosticKind::TypeComplexity, "{source}");

        let narrow_components = TypeLimits {
            max_components: 1,
            ..TypeLimits::default()
        };
        let diagnostic = scarp::compile_with_type_limits(source, narrow_components).unwrap_err();
        assert_eq!(diagnostic.kind, DiagnosticKind::TypeComplexity, "{source}");
    }
}
