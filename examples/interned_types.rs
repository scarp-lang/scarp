use scarp::types::{FunctionEffect, PrimitiveType, TypeKind, TypeLimits, TypeRegistryBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut types = TypeRegistryBuilder::new(TypeLimits::default())?;
    let integer = PrimitiveType::Integer.id();
    let sequence = types.intern(TypeKind::Sequence {
        element: integer,
        length: None,
    })?;
    let pair = types.intern(TypeKind::Tuple(vec![integer, sequence].into()))?;
    let signature = types.intern(TypeKind::Function {
        parameters: vec![pair].into(),
        result: PrimitiveType::Boolean.id(),
        effect: FunctionEffect::Pure,
    })?;
    let snapshot = types.finish();
    assert_eq!(snapshot.len(), 11);
    assert_eq!(snapshot.depth(signature), Some(4));
    println!(
        "types: {}, signature depth: {}",
        snapshot.len(),
        snapshot.depth(signature).unwrap()
    );
    let program = scarp::compile(
        "pure funcra twice(n: integer)->integer{return n*2;} bind answer=twice(21); print(answer);",
    )?;
    assert_eq!(program.type_registry().len(), 9);
    assert_eq!(program.typed_expressions().count(), 3);
    for expression in program.typed_expressions() {
        assert!(expression
            .nodes()
            .all(|node| program.type_registry().get(node.kind).is_some()));
    }
    let mut output = Vec::new();
    scarp::interpreter::execute(&program, &mut output)?;
    assert_eq!(output, b"42\n");
    println!(
        "compiled types: {}, expressions: {}",
        program.type_registry().len(),
        program.typed_expressions().count()
    );
    Ok(())
}
