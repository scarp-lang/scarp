//! Structural source types, portable shapes and arena-local interned identities.
use scarp::{
    types::{PrimitiveType, SourceType, TypeKind},
    value::{TupleValue, Value},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source="pure funcra echo(value:tuple<integer,string>)->tuple<integer,string>{return value;} bind a=(7,\"kept\"); print(echo(a));";
    let program = scarp::compile(source)?;
    let shape = SourceType::tuple(vec![
        PrimitiveType::Integer.into(),
        PrimitiveType::String.into(),
    ])
    .expect("bounded type");
    assert_eq!(program.functions()[0].result, shape);
    let id = program.binding_type_ids()[0];
    assert!(matches!(
        program.type_registry().get(id),
        Some(TypeKind::Tuple(_))
    ));
    let host = Value::Tuple(
        TupleValue::new(vec![Value::Integer(7), Value::from("kept")]).expect("bounded tuple"),
    );
    assert_eq!(host.kind(), shape);
    assert_eq!(host.to_string(), "(7, \"kept\")");
    let mut out = Vec::new();
    scarp::interpreter::execute(&program, &mut out)?;
    assert_eq!(out, b"(7, \"kept\")\n");
    Ok(())
}
