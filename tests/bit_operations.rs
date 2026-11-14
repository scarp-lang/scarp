use scarp::{
    diagnostics::{Diagnostic, DiagnosticKind},
    RunError,
};
fn output(source: &str) -> String {
    let mut bytes = Vec::new();
    scarp::run(source, &mut bytes).unwrap();
    String::from_utf8(bytes).unwrap()
}
fn error(source: &str) -> Diagnostic {
    let expected = scarp::check(source).unwrap_err();
    let mut bytes = Vec::new();
    let RunError::Diagnostic(actual) = scarp::run(source, &mut bytes).unwrap_err() else {
        panic!("expected diagnostic")
    };
    assert_eq!(actual, expected);
    assert!(bytes.is_empty());
    actual
}

#[test]
fn masks_radix_literals_and_precedence_work_together() {
    assert_eq!(output("println(0xA5 & 0b1100); println(1 << 2 + 1 | 3); println(8 | 4 ^ 6 & 3); println(0xff & 1 == 1 and true); println(~0);"),"4\n11\n14\ntrue\n-1\n");
}
#[test]
fn shifts_preserve_defined_sixty_four_bit_patterns() {
    assert_eq!(output("println(1 << 63); println(-8 >> 1); println(-1 >>> 1); println(-1 >>> 63); println(-1 << 1); println(7 >> 0);"),"-9223372036854775808\n-4\n9223372036854775807\n1\n-2\n7\n");
    for expression in ["1 << -1", "1 >> 64", "1 >>> 9223372036854775807"] {
        assert_eq!(
            error(&format!("println({expression});")).kind,
            DiagnosticKind::InvalidShift
        );
    }
}
#[test]
fn compound_bit_operations_and_remainder_keep_snapshots() {
    assert_eq!(output("var flags = 0xff; bind copy = flags; flags &= 0xf0; flags ^= 0x30; flags |= 0x0f; flags <<= 1; flags >>= 1; flags >>>= 1; flags %= 97; println(copy); println(flags);"),"255\n6\n");
    assert_eq!(output("println(-7 % 3); println(7 % -3);"), "-1\n1\n");
    assert_eq!(
        error("println(1 % 0);").kind,
        DiagnosticKind::DivisionByZero
    );
    assert_eq!(
        error("println(-9223372036854775808 % -1);").kind,
        DiagnosticKind::ArithmeticOverflow
    );
}
#[test]
fn integer_only_operations_reject_other_kinds_before_execution() {
    for expression in [
        "1.0 & 2.0",
        "1 | true",
        "1.0 << 1",
        "1 << 1.0",
        "~true",
        "1.0 % 2.0",
    ] {
        assert_eq!(
            error(&format!("println({expression});")).kind,
            DiagnosticKind::TypeMismatch
        );
    }
    assert_eq!(
        error("var x = 1.0; x <<= 1;").kind,
        DiagnosticKind::TypeMismatch
    );
    assert_eq!(
        error("bind x = 1; x |= 2;").kind,
        DiagnosticKind::ImmutableAssignment
    );
}
#[test]
fn radix_literals_keep_signed_range_and_direct_minimum_rules() {
    assert_eq!(output("println(0Xff); println(0B1010); println(-0x8000000000000000); println(-0b1000000000000000000000000000000000000000000000000000000000000000);"),"255\n10\n-9223372036854775808\n-9223372036854775808\n");
    for literal in [
        "0x",
        "0b",
        "0xg",
        "0b2",
        "0b10_10",
        "0x8000000000000000",
        "0xffffffffffffffff",
        "0x10000000000000000",
    ] {
        assert_eq!(
            error(&format!("println({literal});")).kind,
            DiagnosticKind::InvalidInteger,
            "{literal}"
        );
    }
}
#[test]
fn selected_shift_errors_preserve_atomic_output_and_source_spans() {
    assert_eq!(output("println(false and 1 << 64 == 0);"), "false\n");
    let source = "println(1); var x = 1; x >>>= 64;";
    let diagnostic = error(source);
    assert_eq!(diagnostic.kind, DiagnosticKind::InvalidShift);
    assert_eq!(&source[diagnostic.span.start..diagnostic.span.end], ">>>=");
}
#[test]
fn longest_match_keeps_shift_and_assignment_tokens_distinct() {
    use scarp::lexer::{lex, TokenKind};
    let tokens = lex(">>>= >>> >>= >> >= > <<= << <= < &= |= ^= %=").unwrap();
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TokenKind::UnsignedEqual,
            TokenKind::UnsignedRight,
            TokenKind::RightEqual,
            TokenKind::ShiftRight,
            TokenKind::GreaterEqual,
            TokenKind::Greater,
            TokenKind::LeftEqual,
            TokenKind::ShiftLeft,
            TokenKind::LessEqual,
            TokenKind::Less,
            TokenKind::AndEqual,
            TokenKind::OrEqual,
            TokenKind::XorEqual,
            TokenKind::PercentEqual,
            TokenKind::Eof
        ]
    );
}
#[test]
fn nibble_packing_and_unpacking_agree_with_arithmetic_values() {
    let mut source = String::new();
    let mut expected = String::new();
    for value in 0..256 {
        let high = value / 16;
        let low = value % 16;
        source.push_str(&format!("{{ bind packed = ({high} << 4) | {low}; println(packed); println(packed >>> 4 == {high}); }}"));
        expected.push_str(&format!("{value}\ntrue\n"));
    }
    assert_eq!(output(&source), expected);
}
#[test]
fn long_bit_chains_and_gcd_loops_remain_iterative() {
    assert_eq!(
        output(&format!("println(0{});", " | 1".repeat(20000))),
        "1\n"
    );
    assert_eq!(output("var a = 84; var b = 30; while b != 0 { bind remainder = a % b; a = b; b = remainder; } println(a);"),"6\n");
}

#[test]
fn all_shift_counts_match_rust_fixed_width_operations() {
    let mut source = String::new();
    let mut expected = String::new();
    for value in [i64::MIN, i64::MAX, -1, 0, 1, 0x123456789abcdef] {
        for count in 0..64 {
            source.push_str(&format!(
                "print({value}<<{count},{value}>>{count},{value}>>>{count});"
            ));
            expected.push_str(&format!(
                "{} {} {}\n",
                value.wrapping_shl(count),
                value >> count,
                ((value as u64) >> count) as i64
            ));
        }
    }
    assert_eq!(output(&source), expected);
}

#[test]
fn bit_precedence_nesting_fits_the_existing_small_parser_stack() {
    use scarp::parser::{MAX_BLOCK_DEPTH, MAX_EXPRESSION_DEPTH};
    let syntax = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let source = format!(
                "{}print({}1{});{}",
                "{".repeat(MAX_BLOCK_DEPTH),
                "1|(".repeat(MAX_EXPRESSION_DEPTH),
                ")".repeat(MAX_EXPRESSION_DEPTH),
                "}".repeat(MAX_BLOCK_DEPTH)
            );
            let brace_only = format!(
                "{}print(1);{}",
                "{".repeat(MAX_BLOCK_DEPTH),
                "}".repeat(MAX_BLOCK_DEPTH)
            );
            use std::io::Write as _;
            std::io::stderr()
                .write_all(b"SCARP_STACK_STAGE:braces_only\n")
                .unwrap();
            std::mem::forget(scarp::parse(&brace_only).unwrap());
            let expression_only = format!(
                "print({}1{});",
                "1|(".repeat(MAX_EXPRESSION_DEPTH),
                ")".repeat(MAX_EXPRESSION_DEPTH)
            );
            std::io::stderr()
                .write_all(b"SCARP_STACK_STAGE:expression_only\n")
                .unwrap();
            std::mem::forget(scarp::parse(&expression_only).unwrap());
            std::io::stderr()
                .write_all(b"SCARP_STACK_STAGE:combined\n")
                .unwrap();
            let program = scarp::parse(&source).unwrap();
            std::io::stderr()
                .write_all(b"SCARP_STACK_STAGE:combined_returned\n")
                .unwrap();
            program
        })
        .unwrap()
        .join()
        .unwrap();
    let program = scarp::resolver::resolve(syntax).unwrap();
    let mut output = Vec::new();
    scarp::interpreter::execute(&program, &mut output).unwrap();
    assert_eq!(output, b"1\n");
}

#[test]
fn module_constants_fold_bit_operations_and_keep_selected_branch_safety() {
    use scarp::modules::{self, MemorySourceLoader, ModuleLimits};
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"mask.srp\" as mask; print(mask.bits,mask.safe,mask.flip);",
        )
        .unwrap();
    loader.insert("mask.srp","pub bind bits=(0xF0 | 0b11)>>>4; pub bind safe=false and (1<<64)==0; pub bind flip=~0x0;").unwrap();
    let program =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    program
        .execute(&mut out, &mut err, scarp::ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"15 false -1\n");
    assert!(err.is_empty());
}

#[test]
fn every_compound_rejects_wrong_target_and_rhs_types_statically() {
    for operator in ["%=", "&=", "|=", "^=", "<<=", ">>=", ">>>="] {
        for (target, right) in [
            ("1.0", "1"),
            ("1", "1.0"),
            ("true", "1"),
            ("[integer:1]", "1"),
        ] {
            let diagnostic =
                scarp::compile(&format!("var value={target}; value{operator}{right};"))
                    .unwrap_err();
            assert_eq!(diagnostic.kind, DiagnosticKind::TypeMismatch);
            assert!(!diagnostic.related.is_empty());
        }
    }
    assert_eq!(output("pure funcra mask(v: integer=0xF0)->integer ensures result>=0; {return v & 0xF;} print(mask(),mask(0b10101));"),"0 5\n");
}

#[test]
fn cli_and_embedding_agree_on_exact_shift_and_radix_diagnostics() {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bits");
    for name in ["shift", "radix"] {
        let filename = format!("{name}.srp");
        let source = std::fs::read_to_string(folder.join(&filename)).unwrap();
        let expected = std::fs::read_to_string(folder.join(format!("{name}.stderr"))).unwrap();
        assert_eq!(
            format!("{}\n", error(&source).render(&filename, &source)),
            expected
        );
        for mode in ["run", "check"] {
            let result = std::process::Command::new(env!("CARGO_BIN_EXE_scarp"))
                .current_dir(&folder)
                .args([mode, &filename])
                .output()
                .unwrap();
            assert_eq!(result.status.code(), Some(1));
            assert!(result.stdout.is_empty());
            assert_eq!(String::from_utf8(result.stderr).unwrap(), expected);
        }
    }
}
