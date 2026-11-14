use scarp::data_engine::{
    Constraint as C, DataError as E, DataLimits, DataResource as R, DataType as T,
    Definition, Field, GenericSignature as Signature, MapKeyType as K,
    SchemaRegistry as Registry, SpecializationCache as Cache, TypeExpression as X,
    VariantCase,
};
use scarp::types::{DefinitionId as Id, PrimitiveType as P, SourceType};

fn id(n: u32) -> Id { Id { module: 21, declaration: n } }
fn identity() -> Signature {
    Signature {
        identity: id(50),
        type_parameters: 1,
        inputs: vec![X::var(0)],
        result: X::var(0),
        constraints: vec![],
    }
}
fn factory() -> Registry {
    let mut r = Registry::default();
    r.register(Definition::record(id(1),"Student",vec![
        Field::new("name",T::string()),Field::new("age",T::integer()),
    ])).unwrap();
    r.register(Definition::variant(id(2),"Status",vec![
        VariantCase::unit("Pending"),
        VariantCase::typed("Ready",T::Record(id(1))),
    ])).unwrap();
    r
}
#[test]
fn generic_identity_infers_real_source_type() {
    let r=Registry::default();
    let result=identity().infer(&r,&[T::integer()]).unwrap();
    assert_eq!(result.types,vec![T::integer()]);
    assert_eq!(result.result,T::integer());
    assert_eq!(result.identity,id(50));
}
#[test]
fn nested_generic_lists_specialize_without_type_widening() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::list(X::var(0))],
        result:X::optional(X::var(0)),
        constraints:vec![],
    };
    let inferred=s.infer(&r,&[T::list(T::boolean())]).unwrap();
    assert_eq!(inferred.types,vec![T::boolean()]);
    assert_eq!(inferred.result,T::optional(T::boolean()));
    assert_eq!(s.infer(&r,&[T::integer()]),Err(E::TypeMismatch));
}
#[test]
fn repeated_parameter_occurrence_must_unify_to_exact_type() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0),X::var(0)],
        result:X::var(0),
        constraints:vec![],
    };
    assert_eq!(s.infer(&r,&[T::integer(),T::integer()]).unwrap().result,T::integer());
    assert_eq!(s.infer(&r,&[T::integer(),T::string()]),Err(E::TypeMismatch));
    assert_eq!(s.infer(&r,&[T::integer()]),Err(E::ArityMismatch));
}
#[test]
fn two_parameters_preserve_inferred_order_and_outcome_shape() {
    let r=Registry::default();
    let s=Signature {
        identity:id(51),type_parameters:2,
        inputs:vec![X::var(1),X::var(0)],
        result:X::outcome(X::var(0),X::var(1)),
        constraints:vec![],
    };
    let inferred=s.infer(&r,&[T::string(),T::integer()]).unwrap();
    assert_eq!(inferred.types,vec![T::integer(),T::string()]);
    assert_eq!(inferred.result,T::outcome(T::integer(),T::string()));
}
#[test]
fn concrete_templates_are_not_variables_or_coercions() {
    let r=Registry::default();
    let s=Signature {
        identity:id(52),type_parameters:0,
        inputs:vec![X::concrete(T::integer())],
        result:X::concrete(T::boolean()),
        constraints:vec![],
    };
    assert_eq!(s.infer(&r,&[T::integer()]).unwrap().result,T::boolean());
    assert_eq!(s.infer(&r,&[T::string()]),Err(E::TypeMismatch));
    assert_eq!(s.infer(&r,&[T::Source(SourceType::Primitive(P::Float))]),
        Err(E::TypeMismatch));
}
#[test]
fn distinct_nominal_records_do_not_unify_by_shape() {
    let mut r=factory();
    r.register(Definition::record(id(3),"Teacher",vec![
        Field::new("name",T::string()),Field::new("age",T::integer()),
    ])).unwrap();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0),X::var(0)],
        result:X::var(0),
        constraints:vec![],
    };
    assert_eq!(s.infer(&r,&[T::Record(id(1)),T::Record(id(3))]),Err(E::TypeMismatch));
}
#[test]
fn numeric_constraint_accepts_integer_float_bigint_only() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::list(X::var(0)),
        constraints:vec![(0,C::Numeric)],
    };
    for p in [P::Integer,P::Float,P::BigInt] {
        assert_eq!(s.infer(&r,&[T::Source(p.into())]).unwrap().result,
            T::list(T::Source(p.into())));
    }
    assert_eq!(s.infer(&r,&[T::string()]),Err(E::UnsatisfiedConstraint));
    assert_eq!(s.infer(&r,&[T::boolean()]),Err(E::UnsatisfiedConstraint));
}
#[test]
fn map_key_constraint_disallows_non_integer_non_text_types() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::var(0),
        constraints:vec![(0,C::ValidMapKey)],
    };
    assert!(s.infer(&r,&[T::integer()]).is_ok());
    assert!(s.infer(&r,&[T::string()]).is_ok());
    assert_eq!(s.infer(&r,&[T::boolean()]),Err(E::UnsatisfiedConstraint));
    assert_eq!(s.infer(&r,&[T::list(T::integer())]),Err(E::UnsatisfiedConstraint));
}
#[test]
fn nominal_field_and_case_constraints_inspect_registry() {
    let r=factory();
    let field=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::var(0),
        constraints:vec![(0,C::RecordWithField("age".into()))],
    };
    assert!(field.infer(&r,&[T::Record(id(1))]).is_ok());
    assert_eq!(field.infer(&r,&[T::Variant(id(2))]),Err(E::UnsatisfiedConstraint));
    let case=Signature {
        identity:id(51),
        constraints:vec![(0,C::VariantWithCase("Ready".into()))],
        ..field
    };
    assert!(case.infer(&r,&[T::Variant(id(2))]).is_ok());
    assert_eq!(case.infer(&r,&[T::Record(id(1))]),Err(E::UnsatisfiedConstraint));
}
#[test]
fn collection_constraint_rejects_optional_and_scalar_values() {
    let r=Registry::default();
    let sig=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::var(0),
        constraints:vec![(0,C::Collection)],
    };
    assert!(sig.infer(&r,&[T::list(T::string())]).is_ok());
    assert!(sig.infer(&r,&[T::map(K::Integer,T::string())]).is_ok());
    assert_eq!(sig.infer(&r,&[T::optional(T::integer())]),Err(E::UnsatisfiedConstraint));
}
#[test]
fn unused_result_only_parameter_is_an_error() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::concrete(T::integer())],
        result:X::var(0),
        constraints:vec![],
    };
    assert_eq!(s.infer(&r,&[T::integer()]),Err(E::UnboundParameter));
}
#[test]
fn out_of_range_parameter_rejected_in_preflight() {
    let r=Registry::default();
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(1)],
        result:X::var(0),
        constraints:vec![],
    };
    assert_eq!(s.validate(&r),Err(E::UnboundParameter));
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::var(0),
        constraints:vec![(1,C::Numeric)],
    };
    assert_eq!(s.validate(&r),Err(E::UnboundParameter));
}
#[test]
fn input_type_expression_depth_and_nodes_are_bounded() {
    let s=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::list(X::optional(X::list(X::var(0))))],
        result:X::var(0),
        constraints:vec![],
    };
    let r=Registry::new(DataLimits {max_type_depth:3,..DataLimits::default()});
    assert_eq!(s.validate(&r),Err(E::Limit{resource:R::TypeDepth,limit:3}));
    let r=Registry::new(DataLimits {max_type_nodes:2,..DataLimits::default()});
    assert_eq!(s.validate(&r),Err(E::Limit{resource:R::TypeNodes,limit:2}));
}
#[test]
fn generic_specialization_cache_reuses_exact_signature_and_inputs() {
    let r=Registry::default();
    let mut cache=Cache::new(2);
    assert!(cache.is_empty());
    let first=cache.resolve(&r,&identity(),&[T::integer()]).unwrap();
    let second=cache.resolve(&r,&identity(),&[T::integer()]).unwrap();
    assert_eq!(first,second);
    assert_eq!(cache.len(),1);
    cache.resolve(&r,&identity(),&[T::string()]).unwrap();
    assert_eq!(cache.len(),2);
    assert_eq!(cache.resolve(&r,&identity(),&[T::boolean()]),Err(E::Limit{
        resource:R::Specializations,limit:2
    }));
    cache.clear();
    assert!(cache.is_empty());
}
#[test]
fn cache_key_includes_full_signature_not_only_definition_identity() {
    let r=Registry::default();
    let mut cache=Cache::new(5);
    let first=identity();
    let mut changed=identity();
    changed.result=X::list(X::var(0));
    assert_eq!(cache.resolve(&r,&first,&[T::integer()]).unwrap().result,T::integer());
    assert_eq!(cache.resolve(&r,&changed,&[T::integer()]).unwrap().result,T::list(T::integer()));
    assert_eq!(cache.len(),2);
}
#[test]
fn cache_key_includes_schema_contents_not_only_nominal_identity() {
    let mut a=Registry::default();
    a.register(Definition::record(id(1),"A",vec![Field::new("name",T::string())])).unwrap();
    let mut b=Registry::default();
    b.register(Definition::record(id(1),"B",vec![Field::new("age",T::integer())])).unwrap();
    let sig=identity();
    let mut cache=Cache::new(3);
    assert_eq!(cache.resolve(&a,&sig,&[T::Record(id(1))]).unwrap().result,T::Record(id(1)));
    assert_eq!(cache.resolve(&b,&sig,&[T::Record(id(1))]).unwrap().result,T::Record(id(1)));
    assert_eq!(cache.len(),2);
}
#[test]
fn generic_source_structural_types_are_preserved() {
    let r=Registry::default();
    let pair=SourceType::tuple(vec![
        P::Integer.into(),P::String.into(),
    ]).unwrap();
    let want=T::Source(pair.clone());
    let inferred=identity().infer(&r,&[want.clone()]).unwrap();
    assert_eq!(inferred.result,want);
}
#[test]
fn invalid_constraint_name_is_checked_even_when_unused() {
    let r=Registry::default();
    let mut sig=identity();
    sig.constraints=vec![(0,C::RecordWithField("bad-name".into()))];
    assert_eq!(sig.validate(&r),Err(E::InvalidName("bad-name".into())));
}
#[test]
fn generic_map_inference_preserves_integer_text_key_distinction() {
    let r=Registry::default();
    let sig=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::map(K::Text,X::var(0))],
        result:X::optional(X::var(0)),
        constraints:vec![],
    };
    assert_eq!(sig.infer(&r,&[T::map(K::Text,T::boolean())]).unwrap().result,
        T::optional(T::boolean()));
    assert_eq!(sig.infer(&r,&[T::map(K::Integer,T::boolean())]),Err(E::TypeMismatch));
}
#[test]
fn signatures_with_nested_outputs_are_checked_after_substitution() {
    let r=Registry::new(DataLimits{max_type_depth:3,..DataLimits::default()});
    let sig=Signature {
        identity:id(50),type_parameters:1,
        inputs:vec![X::var(0)],
        result:X::list(X::var(0)),
        constraints:vec![],
    };
    let arg=T::optional(T::optional(T::integer()));
    assert_eq!(sig.infer(&r,&[arg]),Err(E::Limit{resource:R::TypeDepth,limit:3}));
}
#[test]
fn specialization_result_is_portable_across_registry_clones() {
    let r=factory();
    let clone=r.clone();
    let original=identity().infer(&r,&[T::Record(id(1))]).unwrap();
    let copy=identity().infer(&clone,&[T::Record(id(1))]).unwrap();
    assert_eq!(copy,original);
}
