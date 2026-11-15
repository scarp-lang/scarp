//! Structural tuple and fixed-array bindings, from parsing through execution.
use scarp::{
    ast::{PatternKind, Statement},
    diagnostics::{Diagnostic, DiagnosticKind as Kind},
    interpreter,
    modules::{
        self,
        cache::{SyntaxCache, SyntaxCacheLimits},
        symbols::IndexLimits,
        MemorySourceLoader, ModuleLimits,
    },
    types::{MAX_COMPOSITE_DEPTH, MAX_EXPANDED_VALUE_NODES},
    ExecutionOptions, RunError,
};

fn run(source: &str) -> Vec<u8> {
    let mut out = Vec::new();
    scarp::run(source, &mut out).unwrap();
    out
}
fn check_error(source: &str, expected: Kind) -> Diagnostic {
    let error = scarp::check(source).unwrap_err();
    let mut output = Vec::new();
    let RunError::Diagnostic(runtime) = scarp::run(source, &mut output).unwrap_err() else {
        panic!("expected a compilation diagnostic: {source}");
    };
    assert_eq!(error.kind, expected, "unexpected error: {error:?}");
    assert_eq!(runtime.kind, error.kind);
    assert_eq!(runtime.span, error.span);
    assert!(output.is_empty(), "no output after a rejected program");
    error
}

#[test]
fn source_tuple_destructuring_runs_and_preserves_heterogeneous_types() {
    assert_eq!(
        run(r#"bind (number, message, active) = (12, "hello", true);
               print(number, message, active);"#),
        b"12 hello true\n"
    );
}
#[test]
fn source_array_destructuring_runs_with_homogeneous_values() {
    assert_eq!(
        run(r#"bind [first, second, third] = [array: 4, 5, 6];
               print(first + second, third);"#),
        b"9 6\n"
    );
}
#[test]
fn source_singleton_tuple_requires_a_comma() {
    assert_eq!(run(r#"bind (item,) = (42,); print(item);"#), b"42\n");
    assert_eq!(
        check_error("bind (item) = (42,);", Kind::UnexpectedToken).kind,
        Kind::UnexpectedToken
    );
}
#[test]
fn source_singleton_array_is_not_a_grouped_tuple() {
    assert_eq!(run("bind [item] = [array: 42]; print(item);"), b"42\n");
}
#[test]
fn mutable_destructured_slots_follow_existing_assignment_rules() {
    assert_eq!(
        run(r#"var (number, word) = (2, "yes");
               number += 3; word = "changed"; print(number, word);
               var [a, b] = [array: 3, 7];
               a += b; print(a, b);"#),
        b"5 changed\n10 7\n"
    );
    check_error(
        "bind (first, second) = (1, 2); first = 4;",
        Kind::ImmutableAssignment,
    );
}
#[test]
fn wildcard_discards_values_without_binding_or_hidden_slots() {
    let compiled =
        scarp::compile(r#"bind (_, second, _) = (true, 9, "unused"); print(second);"#).unwrap();
    assert_eq!(compiled.binding_count(), 1);
    let mut out = Vec::new();
    interpreter::execute(&compiled, &mut out).unwrap();
    assert_eq!(out, b"9\n");
    check_error("bind (x, _) = (1, 2); print(_);", Kind::UnknownName);
}
#[test]
fn multiple_wildcards_still_require_exact_arity() {
    assert_eq!(run("bind (_, _) = (false, true); print(1);"), b"1\n");
    check_error("bind (_, _) = (1, 2, 3);", Kind::TypeMismatch);
}
#[test]
fn tuple_pattern_rejects_fixed_array_initializer_and_vice_versa() {
    check_error("bind (x, y) = [array: 1, 2];", Kind::TypeMismatch);
    check_error("bind [x, y] = (1, 2);", Kind::TypeMismatch);
}
#[test]
fn fixed_integer_vector_is_not_a_typed_fixed_array() {
    check_error("bind [x, y] = [integer: 1, 2];", Kind::TypeMismatch);
}
#[test]
fn wrong_tuple_and_array_lengths_fail_before_output() {
    check_error("bind (x, y, z) = (1, 2);", Kind::TypeMismatch);
    check_error("bind [x, y] = [array: 1];", Kind::TypeMismatch);
    check_error("print(99); bind (x, y) = (1,);", Kind::TypeMismatch);
}
#[test]
fn nested_tuple_values_remain_first_class_after_flat_unpacking() {
    assert_eq!(
        run(r#"bind (pair, message) = ((1, true), "ok");
               print(pair[0], pair[1], message);"#),
        b"1 true ok\n"
    );
}
#[test]
fn fixed_arrays_can_contain_tuple_elements() {
    assert_eq!(
        run(r#"bind [first, second] = [array: (1, true), (2, false)];
               print(first[0], second[1]);"#),
        b"1 false\n"
    );
}
#[test]
fn initializer_executes_only_once_even_with_several_bindings() {
    let source = r#"
        funcra create() -> tuple<integer, integer> {
            print("created");
            return (5, 7);
        }
        bind (left, right) = create();
        print(left + right);
    "#;
    assert_eq!(run(source), b"created\n12\n");
}
#[test]
fn function_arguments_can_be_unpacked_locally() {
    let source = r#"
        pure funcra combine(item: tuple<integer, integer>) -> integer {
            bind (left, right) = item;
            return left * 10 + right;
        }
        print(combine((3, 4)));
    "#;
    assert_eq!(run(source), b"34\n");
}
#[test]
fn function_call_graph_discovers_calls_inside_destructure_initializers() {
    let source = r#"
        pure funcra pair() -> tuple<integer, integer> {
            return (1, 2);
        }
        pure funcra consume() -> integer {
            bind (a, b) = pair();
            return a + b;
        }
        print(consume());
    "#;
    let program = scarp::compile(source).unwrap();
    let consume = program
        .functions()
        .iter()
        .find(|f| f.name == "consume")
        .unwrap();
    assert!(consume.callees.contains(&"pair".to_owned()));
    assert_eq!(run(source), b"3\n");
}
#[test]
fn repeated_execution_of_precompiled_program_uses_fresh_storage() {
    let compiled = scarp::compile("var [x, y] = [array: 2, 3]; x += y; print(x, y);").unwrap();
    for _ in 0..4 {
        let mut out = Vec::new();
        interpreter::execute(&compiled, &mut out).unwrap();
        assert_eq!(out, b"5 3\n");
    }
}
#[test]
fn destructured_bindings_shadow_outer_names_only_in_inner_scope() {
    assert_eq!(
        run(r#"bind x = 9; {
            bind (x, y) = (1, 2);
            print(x, y);
        }
        print(x);"#),
        b"1 2\n9\n"
    );
}
#[test]
fn nested_block_scope_retirement_discards_all_destructured_slots() {
    let compiled =
        scarp::compile("bind root=1; { bind (x,y)=(2,3); print(x+y); } print(root);").unwrap();
    let mut out = Vec::new();
    interpreter::execute(&compiled, &mut out).unwrap();
    assert_eq!(out, b"5\n1\n");
    assert_eq!(compiled.binding_count(), 3);
}
#[test]
fn duplicate_names_in_same_tuple_or_array_pattern_are_rejected() {
    for source in ["bind (x, x) = (1, 2);", "var [x, y, x] = [array: 1, 2, 3];"] {
        let err = check_error(source, Kind::DuplicateBinding);
        assert!(err.message.contains("more than once"));
    }
}
#[test]
fn duplicate_names_in_current_scope_are_rejected() {
    for source in [
        "bind x=1; bind (x, y) = (2, 3);",
        "bind y=3; var [x, y] = [array: 1, 2];",
    ] {
        check_error(source, Kind::DuplicateBinding);
    }
}
#[test]
fn reserved_binding_names_fail_exactly_as_normal_declarations() {
    for source in [
        "bind (true, item) = (1, 2);",
        "var [if, item] = [array: 1, 2];",
        "bind (print, name) = (1, 2);",
    ] {
        check_error(source, Kind::ReservedName);
    }
}
#[test]
fn initializer_cannot_refer_to_a_later_pattern_name() {
    check_error("bind (a, b) = (a, 2);", Kind::UnknownName);
    check_error("bind [a, b] = [array: a, 2];", Kind::UnknownName);
}
#[test]
fn inner_scope_initializer_can_see_existing_outer_binding() {
    assert_eq!(
        run("bind a=4; { bind (a,b)=(a+1,2); print(a,b); } print(a);"),
        b"5 2\n4\n"
    );
}
#[test]
fn missing_pattern_element_or_brace_reports_a_syntax_error() {
    for source in [
        "bind (,x) = (1, 2);",
        "bind (x,,y) = (1, 2);",
        "bind [x,] = [array: 1, 2];",
        "bind (x,y) (1,2);",
        "var [x,y] = [array: 1, 2]",
    ] {
        assert!(scarp::compile(source).is_err(), "{source}");
    }
}
#[test]
fn empty_patterns_are_rejected_instead_of_accepting_untyped_empty_tuples() {
    check_error("bind () = (1,);", Kind::UnexpectedToken);
    check_error("bind [] = [array: <integer>];", Kind::UnexpectedToken);
}
#[test]
fn ast_retains_preorder_shape_spans_and_mutability() {
    let source = "// नमस्ते 🌍\nvar [(alpha, _), (gamma, delta)] = data;";
    let parsed = scarp::parse(source).unwrap();
    let Statement::Destructure {
        pattern,
        mutable,
        initializer,
        span,
    } = &parsed.statements[0]
    else {
        panic!("expected destructuring statement");
    };
    assert!(*mutable);
    let observed: Vec<_> = pattern
        .nodes()
        .iter()
        .map(|node| (&node.kind, &source[node.span.start..node.span.end]))
        .collect();
    assert_eq!(
        observed,
        vec![
            (&PatternKind::Array(2), "[(alpha, _), (gamma, delta)]"),
            (&PatternKind::Tuple(2), "(alpha, _)"),
            (&PatternKind::Name("alpha".into()), "alpha"),
            (&PatternKind::Discard, "_"),
            (&PatternKind::Tuple(2), "(gamma, delta)"),
            (&PatternKind::Name("gamma".into()), "gamma"),
            (&PatternKind::Name("delta".into()), "delta"),
        ]
    );
    assert_eq!(
        &source[span.start..span.end],
        "var [(alpha, _), (gamma, delta)] = data;"
    );
    assert!(!initializer.nodes().is_empty());
}
#[test]
fn max_array_binding_count_is_enforced_in_parser() {
    let names = (0..129)
        .map(|n| format!("value{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("bind [{names}] = 1;");
    assert_eq!(scarp::parse(&source).unwrap_err().kind, Kind::ArrayLimit);
}
#[test]
fn max_tuple_binding_count_is_enforced_in_parser() {
    let names = (0..33)
        .map(|n| format!("value{n}"))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!("bind ({names}) = 1;");
    assert_eq!(scarp::parse(&source).unwrap_err().kind, Kind::TupleLimit);
}
#[test]
fn typed_array_pattern_supports_whole_source_parameter() {
    let source = r#"
        pure funcra dot(a: array<integer,2>) -> integer {
            bind [first, second] = a;
            return first * second;
        }
        print(dot([array: 3, 4]));
    "#;
    assert_eq!(run(source), b"12\n");
}
#[test]
fn pattern_declaration_count_is_one_source_statement() {
    let p = scarp::compile("bind (a, b) = (1, 2); print(a,b);").unwrap();
    assert_eq!(p.statement_count(), 2);
    assert_eq!(p.binding_count(), 2);
}
#[test]
fn destructure_does_not_change_unsupported_native_backend_contract() {
    let program = scarp::compile("bind (a, b) = (1, 2); print(a+b);").unwrap();
    assert!(scarp::c_backend::emit_program(&program).is_err());
    assert!(
        scarp::native_object::emit_elf64_integer_kernel("bind (a,b)=(1,2); print(a);").is_err()
    );
}
#[test]
fn check_and_run_share_the_same_compiled_destructuring_semantics() {
    let source = r#"bind [a, b] = [array: 4, 6]; print(a+b);"#;
    scarp::check(source).unwrap();
    assert_eq!(run(source), b"10\n");
}
#[test]
fn standard_budget_prevents_unbounded_destructuring_computation() {
    let source = r#"bind (a,b,c,d) = (1,2,3,4); print(a+b+c+d);"#;
    let options = ExecutionOptions {
        limits: scarp::ExecutionLimits {
            max_steps: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut out = Vec::new();
    let result = scarp::run_with_options(source, &mut out, options);
    assert!(result.is_err());
    assert!(out.is_empty());
}

#[test]
fn nested_mixed_patterns_evaluate_once_and_preserve_order_and_types() {
    let source = r#"
        funcra load() -> tuple<array<tuple<integer,string>, 2>, tuple<boolean,integer>> {
            print("loaded");
            return ([array: (3, "ignored"), (4, "boxes")], (true, 2));
        }
        var ([(price, _), (quantity, label)], (_, extra)) = load();
        quantity += extra;
        print(price * quantity, label, extra);
    "#;
    let p = scarp::compile(source).unwrap();
    assert_eq!(p.binding_count(), 4);
    let mut out = Vec::new();
    interpreter::execute(&p, &mut out).unwrap();
    assert_eq!(out, b"loaded\n18 boxes 2\n");
}

#[test]
fn nested_bindings_retire_on_loop_continue_and_break() {
    assert_eq!(
        run(r#"
        var total = 0;
        repeat 4 as index {
            bind ((value,), [increment]) = ((index,), [array: 3]);
            if index == 1 { continue; }
            if index == 3 { break; }
            total += value + increment;
        }
        print(total);
    "#),
        b"8\n"
    );
}

#[test]
fn nested_mismatches_point_to_the_smallest_bad_group() {
    for (source, subpattern, message) in [
        (
            "bind ((value,), tail) = (4, true);",
            "(value,)",
            "needs a tuple",
        ),
        (
            "bind ((value,), tail) = ((4, 5), true);",
            "(value,)",
            "requires 1 elements, found 2",
        ),
        (
            "bind ([value], tail) = ((4,), true);",
            "[value]",
            "needs a fixed array",
        ),
        (
            "bind ([value], tail) = ([array: 4, 5], true);",
            "[value]",
            "requires 1 elements, found 2",
        ),
    ] {
        let error = check_error(source, Kind::TypeMismatch);
        assert_eq!(&source[error.span.start..error.span.end], subpattern);
        assert!(error.message.contains(message), "{error:?}");
    }
    for source in [
        "bind ((a), b) = ((1,), 2);",
        "bind ([a), b) = ([array: 1], 2);",
        "bind ([], b) = ([array: 1], 2);",
        "bind ([a] b) = ([array: 1], 2);",
    ] {
        check_error(source, Kind::UnexpectedToken);
    }
}

#[test]
fn duplicates_across_branches_report_both_binding_tokens() {
    let source = "bind ((item,), [item]) = ((1,), [array: 2]);";
    let error = check_error(source, Kind::DuplicateBinding);
    assert_eq!(error.span.start, source.rfind("item").unwrap());
    assert_eq!(error.related[0].span.start, source.find("item").unwrap());
    check_error(
        "bind ([print], tail) = ([array: 1], 2);",
        Kind::ReservedName,
    );
    check_error(
        "bind ([item], tail) = ([array: item], 2);",
        Kind::UnknownName,
    );
}

#[test]
fn composite_pattern_depth_is_bounded_and_stack_independent() {
    let depth = MAX_COMPOSITE_DEPTH;
    let pattern = format!("{}leaf{}", "[".repeat(depth), "]".repeat(depth));
    let value = format!("{}7{}", "[array: ".repeat(depth), "]".repeat(depth));
    let source = format!("bind {pattern} = {value}; print(leaf);");
    let excessive = format!("bind [{pattern}] = {value};");
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            assert_eq!(run(&source), b"7\n");
            let error = scarp::parse(&excessive).unwrap_err();
            assert_eq!(error.kind, Kind::ArrayLimit);
            assert_eq!(&excessive[error.span.start..error.span.end], "[");
            assert_eq!(error.span.start, "bind ".len() + depth);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn expanded_pattern_size_is_bounded_across_individually_valid_groups() {
    let row = format!("[{}_]", "_,".repeat(127));
    let grid = format!("[{}{row}]", format!("{row},").repeat(127));
    let source = format!("bind [{grid},{grid},{grid},{grid}] = value;");
    let error = scarp::parse(&source).unwrap_err();
    assert_eq!(error.kind, Kind::TypeComplexity);
    assert!(error
        .message
        .contains(&MAX_EXPANDED_VALUE_NODES.to_string()));
}

#[test]
fn work_budget_charges_nested_members_including_discards_before_output() {
    let flat = "print(7); bind (_, _) = ((1, 2), [array: 3, 4]);";
    let nested = "print(7); bind ((_, _), [_, _]) = ((1, 2), [array: 3, 4]);";
    let execute = |source, max_steps| {
        let mut out = Vec::new();
        let result = scarp::run_with_options(
            source,
            &mut out,
            ExecutionOptions {
                limits: scarp::ExecutionLimits {
                    max_steps,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        (result, out)
    };
    let flat_min = (1..=100)
        .find(|&steps| execute(flat, steps).0.is_ok())
        .unwrap();
    let nested_min = (1..=100)
        .find(|&steps| execute(nested, steps).0.is_ok())
        .unwrap();
    assert_eq!(nested_min, flat_min + 4);
    let (result, out) = execute(nested, nested_min - 1);
    let Err(RunError::Diagnostic(error)) = result else {
        panic!("expected step limit");
    };
    assert_eq!(error.kind, Kind::ExecutionLimit);
    assert_eq!(
        &nested[error.span.start..error.span.end],
        &nested["print(7); ".len()..]
    );
    assert!(out.is_empty());
    assert_eq!(execute(nested, nested_min).1, b"7\n");
}

#[test]
fn module_index_connects_references_to_nested_binding_tokens() {
    let mut loader = MemorySourceLoader::new();
    let library = r#"// café 🌍
        pub pure funcra calculate() -> integer {
            var ([(amount, _)], extra) = ([array: (4, false)], 3);
            amount += extra;
            return amount;
        }
    "#;
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.calculate());",
        )
        .unwrap();
    loader.insert("lib.srp", library).unwrap();
    let indexed = modules::compile_indexed_with_loader(
        "main.srp",
        &loader,
        ModuleLimits::default(),
        IndexLimits::default(),
    )
    .unwrap();
    let index = indexed.index();
    let amount = index
        .goto_definition(1, library.rfind("amount").unwrap())
        .unwrap();
    assert_eq!(amount.name.as_ref(), "amount");
    assert!(amount.mutable);
    assert_eq!(
        amount.location.unwrap().span.start,
        library.find("amount").unwrap()
    );
    assert_eq!(index.find_references(amount.id).count(), 2);
    assert!(index
        .symbols()
        .iter()
        .all(|symbol| symbol.name.as_ref() != "_"));
    let mut out = Vec::new();
    indexed
        .program()
        .execute(&mut out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    assert_eq!(out, b"7\n");
}

#[test]
fn nested_mismatch_span_stays_local_to_an_imported_source() {
    let mut loader = MemorySourceLoader::new();
    let library = r#"// नमस्ते 🌍
        pub pure funcra invalid() -> integer { bind ([a], b) = (true, 2); return b; }
    "#;
    loader
        .insert(
            "main.srp",
            "import \"lib.srp\" as lib; print(lib.invalid());",
        )
        .unwrap();
    loader.insert("lib.srp", library).unwrap();
    let error =
        modules::compile_with_loader("main.srp", &loader, ModuleLimits::default()).unwrap_err();
    assert_eq!(error.diagnostic.kind, Kind::TypeMismatch);
    let site = error.sources.locate_span(error.diagnostic.span).unwrap();
    assert_eq!(error.sources.files()[site.source].name, "lib.srp");
    assert_eq!(&library[site.span.start..site.span.end], "[a]");
}

#[test]
fn nested_wildcards_count_toward_syntax_cache_and_index_limits() {
    let flat = "bind (_, _) = ((1, 2), [array: 3, 4]);";
    let nested = "bind ((_, _), [_, _]) = ((1, 2), [array: 3, 4]);";
    let limits = ModuleLimits::default();
    let measure = |source| {
        let mut loader = MemorySourceLoader::new();
        loader.insert("main.srp", source).unwrap();
        let mut cache = SyntaxCache::new(SyntaxCacheLimits::default());
        let graph =
            modules::load_with_syntax_cache("main.srp", &loader, limits, &mut cache).unwrap();
        let indexed = modules::resolve_indexed(graph, IndexLimits::default()).unwrap();
        (
            cache.stats().retained_ast_nodes,
            indexed.index().usage().work,
        )
    };
    let (flat_nodes, flat_work) = measure(flat);
    assert_eq!(measure(nested), (flat_nodes + 4, flat_work + 4));

    let mut loader = MemorySourceLoader::new();
    loader.insert("main.srp", nested).unwrap();
    let mut cache = SyntaxCache::new(SyntaxCacheLimits {
        max_ast_nodes: flat_nodes,
        ..SyntaxCacheLimits::default()
    });
    let graph = modules::load_with_syntax_cache("main.srp", &loader, limits, &mut cache).unwrap();
    assert_eq!(cache.stats().entries, 0);
    assert_eq!(cache.stats().bypasses, 1);
    let failure = modules::resolve_indexed(
        graph,
        IndexLimits {
            max_work: flat_work,
            ..IndexLimits::default()
        },
    )
    .unwrap_err();
    assert_eq!(failure.diagnostic.kind, Kind::SymbolIndexLimit);
}
