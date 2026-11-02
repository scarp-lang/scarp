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
    Ok(())
}
