use scarp::{
    diagnostics::DiagnosticKind as Kind,
    numeric::{self, Builtin},
    value::{NumericTile, NumericVector, Value, MAX_VECTOR_LENGTH},
    RunError,
};

fn vec_value(values: &[f64]) -> Value {
    Value::Vector(NumericVector::new(values).unwrap())
}

fn tile_value(values: &[f64], height: usize, width: usize) -> Value {
    Value::Tile(NumericTile::new(values, height, width).unwrap())
}

fn call(kernel: Builtin, args: &[Value]) -> Value {
    numeric::evaluate(kernel, args, u64::MAX).unwrap()
}

#[test]
fn rectangular_outer_product_has_correct_orientation() {
    assert_eq!(
        call(
            Builtin::Outer,
            &[vec_value(&[2.0, -3.0]), vec_value(&[1.0, 4.0, -2.0])]
        ),
        tile_value(&[2.0, 8.0, -4.0, -3.0, -12.0, 6.0], 2, 3)
    );
    assert_eq!(
        call(Builtin::Outer, &[vec_value(&[]), vec_value(&[1.0; 3])]),
        tile_value(&[], 0, 3)
    );
    assert_eq!(
        call(Builtin::Outer, &[vec_value(&[1.0; 3]), vec_value(&[])]),
        tile_value(&[], 3, 0)
    );
}

#[test]
fn maximum_outer_shape_and_over_limit_shapes() {
    let a = vec_value(&vec![2.0; 256]);
    let b = vec_value(&vec![3.0; 256]);
    assert_eq!(
        call(Builtin::Outer, &[a.clone(), b]),
        tile_value(&vec![6.0; MAX_VECTOR_LENGTH], 256, 256)
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(
            Builtin::Outer,
            &[a, vec_value(&vec![1.0; 257])],
            0,
            0
        )
        .unwrap_err()
        .kind,
        Kind::VectorLength
    );
}

#[test]
fn transpose_product_matches_materialized_reference() {
    let a = tile_value(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3);
    let b = vec_value(&[2.0, -1.0]);
    let expected = call(
        Builtin::MatVec,
        &[call(Builtin::Turn, std::slice::from_ref(&a)), b.clone()],
    );
    assert_eq!(call(Builtin::MatVecTranspose, &[a, b]), expected);
    assert_eq!(expected, vec_value(&[-2.0, -1.0, 0.0]));
}

#[test]
fn transpose_product_prepays_only_output_not_a_transpose() {
    let args = [
        tile_value(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3),
        vec_value(&[2.0, 1.0]),
    ];
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::MatVecTranspose, &args, 15, 24).unwrap(),
        vec_value(&[6.0, 9.0, 12.0])
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::MatVecTranspose, &args, 14, 24)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::MatVecTranspose, &args, 15, 23)
            .unwrap_err()
            .kind,
        Kind::VectorBudget
    );
}

#[test]
fn bias_and_tile_arithmetic_preserve_rectangular_shape() {
    let a = tile_value(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3);
    let b = tile_value(&[6.0, 5.0, 4.0, 3.0, 2.0, 1.0], 2, 3);
    assert_eq!(
        call(Builtin::TileBias, &[a.clone(), vec_value(&[10.0, 20.0, 30.0])]),
        tile_value(&[11.0, 22.0, 33.0, 14.0, 25.0, 36.0], 2, 3)
    );
    assert_eq!(
        call(Builtin::TileSub, &[a.clone(), b.clone()]),
        tile_value(&[-5.0, -3.0, -1.0, 1.0, 3.0, 5.0], 2, 3)
    );
    assert_eq!(
        call(Builtin::TileHadamard, &[a.clone(), b]),
        tile_value(&[6.0, 10.0, 12.0, 12.0, 10.0, 6.0], 2, 3)
    );
    assert_eq!(a, tile_value(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3));
}

#[test]
fn axis_reductions_are_compensated_and_shape_aware() {
    let a = tile_value(&[1e16, 1.0, -1e16, 1.0, 2.0, 3.0], 2, 3);
    assert_eq!(
        call(Builtin::RowSums, std::slice::from_ref(&a)),
        vec_value(&[1.0, 6.0])
    );
    let transpose = call(Builtin::Turn, &[a]);
    assert_eq!(
        call(Builtin::ColumnSums, &[transpose]),
        vec_value(&[1.0, 6.0])
    );
}

#[test]
fn empty_dimensions_preserve_axes_and_charge_explicit_zeros() {
    let a = tile_value(&[], 3, 0);
    let b = tile_value(&[], 0, 3);
    assert_eq!(call(Builtin::TileBias, &[a.clone(), vec_value(&[])]), a);
    assert_eq!(
        call(Builtin::TileBias, &[b.clone(), vec_value(&[1.0; 3])]),
        b
    );
    assert_eq!(
        call(Builtin::RowSums, std::slice::from_ref(&a)),
        vec_value(&[0.0; 3])
    );
    assert_eq!(
        call(Builtin::ColumnSums, std::slice::from_ref(&b)),
        vec_value(&[0.0; 3])
    );
    assert_eq!(
        call(Builtin::ColumnSums, std::slice::from_ref(&a)),
        vec_value(&[])
    );
    assert_eq!(
        call(Builtin::RowSums, std::slice::from_ref(&b)),
        vec_value(&[])
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(
            Builtin::MatVecTranspose,
            &[b.clone(), vec_value(&[])],
            3,
            24
        )
        .unwrap(),
        vec_value(&[0.0; 3])
    );
    assert_eq!(
        numeric::evaluate_with_vector_limit(Builtin::ColumnSums, &[b], 2, 24)
            .unwrap_err()
            .kind,
        Kind::ExecutionLimit
    );
}

#[test]
fn equal_cell_counts_do_not_imply_equal_shapes() {
    let a = tile_value(&[1.0; 6], 2, 3);
    let b = tile_value(&[1.0; 6], 3, 2);
    for kernel in [Builtin::TileSub, Builtin::TileHadamard] {
        assert_eq!(
            numeric::evaluate(kernel, &[a.clone(), b.clone()], 0)
                .unwrap_err()
                .kind,
            Kind::VectorLength
        );
        assert_eq!(
            numeric::evaluate(kernel, &[tile_value(&[], 2, 0), tile_value(&[], 0, 2)], 0)
                .unwrap_err()
                .kind,
            Kind::VectorLength
        );
    }
    for kernel in [Builtin::TileBias, Builtin::MatVecTranspose] {
        assert_eq!(
            numeric::evaluate(kernel, &[a.clone(), vec_value(&[1.0])], 0)
                .unwrap_err()
                .kind,
            Kind::VectorLength
        );
    }
}

#[test]
fn all_materializations_obey_exact_work_and_payload_limits() {
    let a = tile_value(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 2, 3);
    let cases = [
        (Builtin::Outer, vec![vec_value(&[1.0; 2]), vec_value(&[1.0; 3])], 6, 48),
        (Builtin::TileSub, vec![a.clone(), a.clone()], 6, 48),
        (Builtin::TileHadamard, vec![a.clone(), a.clone()], 6, 48),
        (Builtin::TileBias, vec![a.clone(), vec_value(&[1.0; 3])], 6, 48),
        (Builtin::RowSums, vec![a.clone()], 8, 16),
        (Builtin::ColumnSums, vec![a], 9, 24),
    ];
    for (kernel, args, work, bytes) in cases {
        let before = args.clone();
        numeric::evaluate_with_vector_limit(kernel, &args, work, bytes).unwrap();
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, work - 1, bytes)
                .unwrap_err()
                .kind,
            Kind::ExecutionLimit
        );
        assert_eq!(
            numeric::evaluate_with_vector_limit(kernel, &args, work, bytes - 1)
                .unwrap_err()
                .kind,
            Kind::VectorBudget
        );
        assert_eq!(args, before);
    }
}

#[test]
fn numeric_overflow_is_atomic_at_source_level() {
    for expr in [
        "outer([1e308], [2.0])",
        "matvec_transpose(tile([1e308], 1, 1), [2.0])",
        "tile_bias(tile([1e308], 1, 1), [1e308])",
        "tile_sub(tile([1e308], 1, 1), tile([-1e308], 1, 1))",
        "tile_hadamard(tile([1e308], 1, 1), tile([2.0], 1, 1))",
        "row_sums(tile([1e308, 1e308], 1, 2))",
        "column_sums(tile([1e308, 1e308], 2, 1))",
    ] {
        let source = format!("print(1); print({expr});");
        let expected = scarp::check(&source).unwrap_err();
        assert_eq!(expected.kind, Kind::NonFiniteNumber);
        let mut out = Vec::new();
        let RunError::Diagnostic(actual) = scarp::run(&source, &mut out).unwrap_err() else {
            panic!("expected a numerical diagnostic");
        };
        assert_eq!(expected, actual);
        assert!(out.is_empty());
    }
}

#[test]
fn generated_rectangles_satisfy_adjoint_identity() {
    for rows in 0..8 {
        for cols in 0..8 {
            let cells: Vec<f64> = (0..rows * cols).map(|i| (i % 11) as f64 - 5.0).collect();
            let a = tile_value(&cells, rows, cols);
            let x = vec_value(&(0..cols).map(|i| i as f64 - 2.0).collect::<Vec<_>>());
            let y = vec_value(&(0..rows).map(|i| 3.0 - i as f64).collect::<Vec<_>>());
            let ax = call(Builtin::MatVec, &[a.clone(), x.clone()]);
            let aty = call(Builtin::MatVecTranspose, &[a, y.clone()]);
            assert_eq!(call(Builtin::Dot, &[ax, y]), call(Builtin::Dot, &[x, aty]));
        }
    }
}

#[test]
fn window_backed_tiles_use_only_visible_cells() {
    let values = NumericVector::new(&[999.0, 1.0, 2.0, 3.0, 4.0, 999.0]).unwrap();
    let view = values.window(1, 4).unwrap();
    let a = Value::Tile(NumericTile::from_vector(view, 2, 2).unwrap());
    assert_eq!(
        call(Builtin::ColumnSums, std::slice::from_ref(&a)),
        vec_value(&[4.0, 6.0])
    );
    assert_eq!(
        call(Builtin::MatVecTranspose, &[a, vec_value(&[1.0, 1.0])]),
        vec_value(&[4.0, 6.0])
    );
}
