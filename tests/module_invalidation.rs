use scarp::{
    diagnostics::DiagnosticKind as Kind,
    modules::{
        self,
        invalidation::{
            EditImpact, InvalidationError, InvalidationLimits, InvalidationSnapshot,
            TransitionImpact,
        },
        MemorySourceLoader, ModuleLimits, ModuleProgram,
    },
    ExecutionOptions,
};

fn build(sources: &[(&str, &str)]) -> Result<ModuleProgram, modules::ModuleDiagnostic> {
    let mut loader = MemorySourceLoader::new();
    for &(identity, text) in sources {
        loader.insert(identity, text).unwrap();
    }
    modules::compile_with_loader("main.srp", &loader, ModuleLimits::default())
}

fn snapshot(program: &ModuleProgram) -> InvalidationSnapshot {
    InvalidationSnapshot::new(program, InvalidationLimits::default()).unwrap()
}

fn names(graph: &InvalidationSnapshot, ids: &[usize]) -> Vec<String> {
    ids.iter()
        .map(|&id| graph.identity(id).unwrap().to_owned())
        .collect()
}

fn diamond(shared: &str) -> ModuleProgram {
    build(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value, b.value);",
        ),
        (
            "a.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        (
            "b.srp",
            "import \"shared.srp\" as s; pub bind value=s.value;",
        ),
        ("shared.srp", shared),
    ])
    .unwrap()
}

#[test]
fn exact_preflight_identities_propagate_through_diamonds() {
    let compiled = diamond("pub bind value=1;");
    let graph = snapshot(&compiled);
    assert_eq!(graph.file_count(), 4);
    assert_eq!(
        names(&graph, &graph.plan_edits(&["shared.srp"]).unwrap().affected),
        ["main.srp", "a.srp", "shared.srp", "b.srp"]
    );
    assert_eq!(
        names(
            &graph,
            &graph.plan_edits(&["b.srp", "b.srp"]).unwrap().affected
        ),
        ["main.srp", "b.srp"]
    );
    assert_eq!(
        graph.plan_edits(&[]),
        Ok(EditImpact {
            affected: Vec::new(),
            unknown_identity: false,
            must_rediscover: false,
        })
    );
    let unknown = graph.plan_edits(&["new.srp"]).unwrap();
    assert!(unknown.unknown_identity && unknown.must_rediscover);
    assert_eq!(unknown.affected, vec![0, 1, 2, 3]);
    let unknown_mixed = graph.plan_edits(&["shared.srp", "renamed.srp"]).unwrap();
    assert_eq!(unknown_mixed, unknown);
    assert_eq!(graph.identity(42), None);
    assert_eq!(graph.identity(0), Some("main.srp"));
}

#[test]
fn exact_content_transition_propagates_constant_edits_and_preserves_unaffected_subtrees() {
    let old = diamond("pub bind value=1;");
    let current = diamond("pub bind value=2;");
    let before = snapshot(&old);
    let after = snapshot(&current);
    let plan = before.compare(&after);
    assert_eq!(plan.removed_old, Vec::<usize>::new());
    assert_eq!(plan.added_new, Vec::<usize>::new());
    assert_eq!(names(&before, &plan.modified_old), ["shared.srp"]);
    assert_eq!(names(&after, &plan.modified_new), ["shared.srp"]);
    assert_eq!(
        names(&before, &plan.affected_old),
        ["main.srp", "a.srp", "shared.srp", "b.srp"]
    );
    assert_eq!(
        names(&after, &plan.affected_new),
        ["main.srp", "a.srp", "shared.srp", "b.srp"]
    );

    let mut old_out = Vec::new();
    let mut new_out = Vec::new();
    old.execute(&mut old_out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    current
        .execute(&mut new_out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    assert_eq!(old_out, b"1 1\n");
    assert_eq!(new_out, b"2 2\n");

    let unchanged = before.compare(&snapshot(&old));
    assert_eq!(
        unchanged,
        TransitionImpact {
            removed_old: vec![],
            added_new: vec![],
            modified_old: vec![],
            modified_new: vec![],
            affected_old: vec![],
            affected_new: vec![],
        }
    );
}

#[test]
fn removal_reorders_source_ids_without_leaking_old_identities() {
    let old = build(&[
        (
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; print(a.value);",
        ),
        ("a.srp", "pub bind value=1;"),
        ("b.srp", "pub bind value=2;"),
    ])
    .unwrap();
    let new = build(&[
        ("main.srp", "import \"a.srp\" as a; print(a.value);"),
        ("a.srp", "pub bind value=1;"),
    ])
    .unwrap();
    let before = snapshot(&old);
    let after = snapshot(&new);
    let impact = before.compare(&after);
    assert_eq!(names(&before, &impact.removed_old), ["b.srp"]);
    assert!(impact.added_new.is_empty());
    assert_eq!(names(&before, &impact.modified_old), ["main.srp"]);
    assert_eq!(names(&after, &impact.modified_new), ["main.srp"]);
    assert_eq!(names(&before, &impact.affected_old), ["main.srp", "b.srp"]);
    assert_eq!(names(&after, &impact.affected_new), ["main.srp"]);
}

#[test]
fn adding_an_import_tracks_added_file_and_affected_importers() {
    let old = build(&[("main.srp", "print(1);")]).unwrap();
    let new = build(&[
        ("main.srp", "import \"math.srp\" as m; print(m.value);"),
        ("math.srp", "pub bind value=1;"),
    ])
    .unwrap();
    let before = snapshot(&old);
    let after = snapshot(&new);
    let impact = before.compare(&after);
    assert_eq!(names(&after, &impact.added_new), ["math.srp"]);
    assert_eq!(names(&before, &impact.modified_old), ["main.srp"]);
    assert_eq!(names(&before, &impact.affected_old), ["main.srp"]);
    assert_eq!(
        names(&after, &impact.affected_new),
        ["main.srp", "math.srp"]
    );
}

#[test]
fn exact_loader_retarget_invalidates_even_if_source_bytes_match() {
    use modules::{SourceLoadError, SourceLoader};

    struct Redirect<'a> {
        loader: &'a MemorySourceLoader,
        target: &'a str,
    }
    impl SourceLoader for Redirect<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            if importer.is_some() && specifier == "lib.srp" {
                return Ok(self.target.to_owned());
            }
            self.loader.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.loader.load(identity, max_bytes)
        }
    }

    let mut loader = MemorySourceLoader::new();
    loader
        .insert("main.srp", "import \"lib.srp\" as l; print(l.value);")
        .unwrap();
    loader.insert("v1.srp", "pub bind value=1;").unwrap();
    loader.insert("v2.srp", "pub bind value=2;").unwrap();
    let old = modules::compile_with_loader(
        "main.srp",
        &Redirect {
            loader: &loader,
            target: "v1.srp",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    let next = modules::compile_with_loader(
        "main.srp",
        &Redirect {
            loader: &loader,
            target: "v2.srp",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    let before = snapshot(&old);
    let after = snapshot(&next);
    let impact = before.compare(&after);
    assert_eq!(names(&before, &impact.removed_old), ["v1.srp"]);
    assert_eq!(names(&after, &impact.added_new), ["v2.srp"]);
    assert_eq!(names(&before, &impact.modified_old), ["main.srp"]);
    assert_eq!(names(&after, &impact.modified_new), ["main.srp"]);
    assert_eq!(names(&before, &impact.affected_old), ["main.srp", "v1.srp"]);
    assert_eq!(names(&after, &impact.affected_new), ["main.srp", "v2.srp"]);
}

#[test]
fn per_import_target_changes_are_detected_even_when_unique_dependencies_are_unchanged() {
    use modules::{SourceLoadError, SourceLoader};

    struct Aliases<'a> {
        loader: &'a MemorySourceLoader,
        second_target: &'a str,
    }
    impl SourceLoader for Aliases<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            if importer.is_some() {
                let target = match specifier {
                    "a.srp" => Some("v1.srp"),
                    "b.srp" => Some(self.second_target),
                    "c.srp" => Some("v2.srp"),
                    _ => None,
                };
                if let Some(target) = target {
                    return Ok(target.to_owned());
                }
            }
            self.loader.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.loader.load(identity, max_bytes)
        }
    }
    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"a.srp\" as a; import \"b.srp\" as b; import \"c.srp\" as c; print(a.value,b.value,c.value);",
        )
        .unwrap();
    loader.insert("v1.srp", "pub bind value=1;").unwrap();
    loader.insert("v2.srp", "pub bind value=2;").unwrap();

    let old = modules::compile_with_loader(
        "main.srp",
        &Aliases {
            loader: &loader,
            second_target: "v1.srp",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    let new = modules::compile_with_loader(
        "main.srp",
        &Aliases {
            loader: &loader,
            second_target: "v2.srp",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    // Both successful graphs have the SAME source text, the same 3 file
    // identities and the same deduplicated main dependencies [v1, v2].
    assert_eq!(old.sources().source(0), new.sources().source(0));
    assert_eq!(old.dependencies(0), new.dependencies(0));
    assert_eq!(old.sources().files().len(), 3);
    assert_eq!(new.sources().files().len(), 3);
    let previous = snapshot(&old);
    let current = snapshot(&new);
    let impact = previous.compare(&current);
    assert!(impact.added_new.is_empty() && impact.removed_old.is_empty());
    assert_eq!(names(&previous, &impact.modified_old), ["main.srp"]);
    assert_eq!(names(&current, &impact.modified_new), ["main.srp"]);
    assert_eq!(names(&previous, &impact.affected_old), ["main.srp"]);
    assert_eq!(names(&current, &impact.affected_new), ["main.srp"]);

    let mut old_out = Vec::new();
    let mut new_out = Vec::new();
    old.execute(&mut old_out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    new.execute(&mut new_out, &mut Vec::new(), ExecutionOptions::default())
        .unwrap();
    assert_eq!(old_out, b"1 1 2\n");
    assert_eq!(new_out, b"1 2 2\n");
}

#[test]
fn changed_display_names_invalidate_diagnostics_even_with_identical_identities_and_text() {
    use modules::{SourceLoadError, SourceLoader};

    struct Display<'a> {
        loader: &'a MemorySourceLoader,
        prefix: &'a str,
    }
    impl SourceLoader for Display<'_> {
        fn resolve(
            &self,
            importer: Option<&str>,
            specifier: &str,
        ) -> Result<String, SourceLoadError> {
            self.loader.resolve(importer, specifier)
        }
        fn load(&self, identity: &str, max_bytes: usize) -> Result<String, SourceLoadError> {
            self.loader.load(identity, max_bytes)
        }
        fn display_name(&self, identity: &str) -> String {
            format!("{}/{identity}", self.prefix)
        }
    }

    let mut loader = MemorySourceLoader::new();
    loader
        .insert(
            "main.srp",
            "import \"lib/café.srp\" as lib; print(lib.value);",
        )
        .unwrap();
    loader.insert("lib/café.srp", "pub bind value=27;").unwrap();
    let original = modules::compile_with_loader(
        "main.srp",
        &Display {
            loader: &loader,
            prefix: "old",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    let renamed = modules::compile_with_loader(
        "main.srp",
        &Display {
            loader: &loader,
            prefix: "new",
        },
        ModuleLimits::default(),
    )
    .unwrap();
    assert_eq!(original.sources().files()[1].name, "old/lib/café.srp");
    assert_eq!(renamed.sources().files()[1].name, "new/lib/café.srp");
    assert_eq!(original.sources().source(1), renamed.sources().source(1));
    let before = snapshot(&original);
    let after = snapshot(&renamed);
    assert_eq!(before.identity(1), after.identity(1));
    assert_eq!(
        names(
            &before,
            &before.plan_edits(&["lib/café.srp"]).unwrap().affected
        ),
        ["main.srp", "lib/café.srp"]
    );
    let impact = before.compare(&after);
    assert_eq!(names(&before, &impact.modified_old), ["lib/café.srp"]);
    assert_eq!(names(&after, &impact.modified_new), ["lib/café.srp"]);
    assert_eq!(
        names(&before, &impact.affected_old),
        ["main.srp", "lib/café.srp"]
    );
    assert_eq!(
        names(&after, &impact.affected_new),
        ["main.srp", "lib/café.srp"]
    );
}

#[test]
fn malformed_new_graph_does_not_supply_a_partial_transition() {
    let current = build(&[
        ("main.srp", "import \"lib.srp\" as l; print(l.value);"),
        ("lib.srp", "pub bind value=1;"),
    ])
    .unwrap();
    let old = snapshot(&current);
    let new = build(&[
        ("main.srp", "import \"lib.srp\" as l; print(l.value);"),
        ("lib.srp", "bind value=1;"),
    ])
    .unwrap_err();
    assert_eq!(new.diagnostic.kind, Kind::ModuleVisibility);
    // Before compiling the invalid source, the host can only invalidate the
    // old graph conservatively. It cannot produce a successful new snapshot.
    assert_eq!(
        names(&old, &old.plan_edits(&["lib.srp"]).unwrap().affected),
        ["main.srp", "lib.srp"]
    );
}

#[test]
fn budgets_bound_graph_construction_and_input_edits() {
    let program = build(&[
        ("main.srp", "import \"lib.srp\" as l; print(l.f());"),
        ("lib.srp", "pub pure funcra f()->integer{return 5;}"),
    ])
    .unwrap();
    let base = InvalidationLimits::default();
    for (limits, expected) in [
        (
            InvalidationLimits {
                max_files: 1,
                ..base
            },
            InvalidationError::FileLimit,
        ),
        (
            InvalidationLimits {
                max_edges: 0,
                ..base
            },
            InvalidationError::EdgeLimit,
        ),
        (
            InvalidationLimits {
                max_identity_bytes: 1,
                ..base
            },
            InvalidationError::IdentityLimit,
        ),
    ] {
        assert!(matches!(
            InvalidationSnapshot::new(&program, limits),
            Err(error) if error == expected
        ));
    }
    let bounded = InvalidationSnapshot::new(
        &program,
        InvalidationLimits {
            max_changed_files: 1,
            ..base
        },
    )
    .unwrap();
    assert_eq!(
        bounded.plan_edits(&["main.srp", "lib.srp"]),
        Err(InvalidationError::EditLimit)
    );
    let short_identity = InvalidationSnapshot::new(
        &program,
        InvalidationLimits {
            max_identity_bytes: 19,
            ..base
        },
    )
    .unwrap();
    assert_eq!(
        short_identity.plan_edits(&["main.srp", "main.srp", "main.srp"]),
        Err(InvalidationError::IdentityLimit)
    );
}

#[test]
fn invalidation_snapshots_are_clonable_across_read_only_threads() {
    let before = snapshot(&diamond("pub bind value=23;"));
    let copied = before.clone();
    let join = std::thread::spawn(move || {
        let plan = copied.plan_edits(&["shared.srp"]).unwrap();
        assert_eq!(plan.affected.len(), 4);
        assert_eq!(copied.compare(&copied).affected_old.len(), 0);
    });
    join.join().unwrap();
    assert_eq!(before.file_count(), 4);
}
