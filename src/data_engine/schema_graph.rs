//! Deterministic nominal schema dependency graph and cycle analysis.
//!
//! Type references form graph edges rather than copied recursive syntax trees.
//! This module supports incremental checking and module/type invalidation while
//! remaining independent of any stable .srp serialization or object layout.
use super::limits::DataError;
use super::schema::{DataType, DefinitionKind, SchemaRegistry};
use crate::types::DefinitionId;
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaGraph {
    identities: Vec<DefinitionId>,
    edges: Vec<Vec<usize>>,
    reverse: Vec<Vec<usize>>,
    indices: HashMap<DefinitionId, usize>,
}

fn references(ty: &DataType, output: &mut Vec<DefinitionId>) {
    let mut stack = vec![ty];
    while let Some(part) = stack.pop() {
        match part {
            DataType::Record(id) | DataType::Variant(id) => output.push(*id),
            DataType::List(inner) | DataType::Map(_, inner) | DataType::Optional(inner) => {
                stack.push(inner)
            }
            DataType::Outcome(ok, err) => {
                stack.push(err);
                stack.push(ok);
            }
            DataType::Source(_) => (),
        }
    }
}

impl SchemaGraph {
    /// Deterministic declaration-order graph. Every reference is checked
    /// against the registry's known nominal identities.
    pub fn new(registry: &SchemaRegistry) -> Result<Self, DataError> {
        let identities: Vec<_> = registry.definitions().iter().map(|d| d.id).collect();
        let indices: HashMap<_, _> = identities
            .iter()
            .enumerate()
            .map(|(n, id)| (*id, n))
            .collect();
        let mut edges = vec![Vec::new(); identities.len()];
        let mut reverse = vec![Vec::new(); identities.len()];
        for (position, definition) in registry.definitions().iter().enumerate() {
            let mut links = Vec::new();
            match &definition.kind {
                DefinitionKind::Record(fields) => {
                    for field in fields {
                        references(&field.ty, &mut links);
                    }
                }
                DefinitionKind::Variant(cases) => {
                    for case in cases {
                        if let Some(ty) = &case.payload {
                            references(ty, &mut links);
                        }
                    }
                }
            }
            let mut unique = BTreeSet::new();
            for id in links {
                let index = *indices.get(&id).ok_or(DataError::UnknownDefinition)?;
                unique.insert(index);
            }
            edges[position] = unique.into_iter().collect();
        }
        for (dependent, targets) in edges.iter().enumerate() {
            for &dependency in targets {
                reverse[dependency].push(dependent);
            }
        }
        Ok(Self {
            identities,
            edges,
            reverse,
            indices,
        })
    }
    pub fn len(&self) -> usize {
        self.identities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.identities.is_empty()
    }
    pub fn identities(&self) -> &[DefinitionId] {
        &self.identities
    }
    pub fn dependencies(&self, id: DefinitionId) -> Result<Vec<DefinitionId>, DataError> {
        let index = self.index(id)?;
        Ok(self.edges[index]
            .iter()
            .map(|&i| self.identities[i])
            .collect())
    }
    pub fn dependents(&self, id: DefinitionId) -> Result<Vec<DefinitionId>, DataError> {
        let index = self.index(id)?;
        Ok(self.reverse[index]
            .iter()
            .map(|&i| self.identities[i])
            .collect())
    }
    fn index(&self, id: DefinitionId) -> Result<usize, DataError> {
        self.indices
            .get(&id)
            .copied()
            .ok_or(DataError::UnknownDefinition)
    }
    fn walk(&self, origin: DefinitionId, reverse: bool) -> Result<Vec<DefinitionId>, DataError> {
        let root = self.index(origin)?;
        let edges = if reverse { &self.reverse } else { &self.edges };
        let mut found = vec![false; self.len()];
        let mut queue = VecDeque::new();
        found[root] = true;
        queue.push_back(root);
        while let Some(index) = queue.pop_front() {
            for &adjacent in &edges[index] {
                if !found[adjacent] {
                    found[adjacent] = true;
                    queue.push_back(adjacent);
                }
            }
        }
        // Return in declaration order, excluding the supplied identity.
        Ok(self
            .identities
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != root && found[*i])
            .map(|(_, id)| *id)
            .collect())
    }
    pub fn transitive_dependencies(
        &self,
        id: DefinitionId,
    ) -> Result<Vec<DefinitionId>, DataError> {
        self.walk(id, false)
    }
    pub fn transitive_dependents(&self, id: DefinitionId) -> Result<Vec<DefinitionId>, DataError> {
        self.walk(id, true)
    }
    pub fn depends_on(
        &self,
        source: DefinitionId,
        target: DefinitionId,
    ) -> Result<bool, DataError> {
        self.index(target)?;
        Ok(self.transitive_dependencies(source)?.contains(&target))
    }
    /// An acyclic graph yields a dependency-first order for staged resolution.
    /// Circular schemas are legal data declarations but do not have an
    /// acyclic lowering order.
    pub fn dependency_order(&self) -> Result<Vec<DefinitionId>, DataError> {
        let mut pending: Vec<_> = self.edges.iter().map(Vec::len).collect();
        let mut ready: BTreeSet<usize> = pending
            .iter()
            .enumerate()
            .filter(|(_, count)| **count == 0)
            .map(|(i, _)| i)
            .collect();
        let mut ordered = Vec::new();
        while let Some(&next) = ready.iter().next() {
            ready.remove(&next);
            ordered.push(self.identities[next]);
            for &dependent in &self.reverse[next] {
                pending[dependent] -= 1;
                if pending[dependent] == 0 {
                    ready.insert(dependent);
                }
            }
        }
        if ordered.len() != self.len() {
            return Err(DataError::CyclicDefinition);
        }
        Ok(ordered)
    }
    /// Iterative Kosaraju SCC traversal avoids Rust call-stack recursion.
    /// Components and their members are returned in declaration order.
    pub fn strongly_connected_components(&self) -> Vec<Vec<DefinitionId>> {
        let n = self.len();
        let mut seen = vec![false; n];
        let mut finish = Vec::with_capacity(n);
        for root in 0..n {
            if seen[root] {
                continue;
            }
            seen[root] = true;
            let mut stack = vec![(root, 0usize)];
            while let Some((node, index)) = stack.last_mut() {
                if *index < self.edges[*node].len() {
                    let next = self.edges[*node][*index];
                    *index += 1;
                    if !seen[next] {
                        seen[next] = true;
                        stack.push((next, 0));
                    }
                } else {
                    finish.push(*node);
                    stack.pop();
                }
            }
        }
        seen.fill(false);
        let mut components: Vec<Vec<usize>> = Vec::new();
        for &root in finish.iter().rev() {
            if seen[root] {
                continue;
            }
            let mut component = Vec::new();
            let mut stack = vec![root];
            seen[root] = true;
            while let Some(node) = stack.pop() {
                component.push(node);
                for &next in self.reverse[node].iter().rev() {
                    if !seen[next] {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
            component.sort_unstable();
            components.push(component);
        }
        components.sort_by_key(|part| part[0]);
        components
            .into_iter()
            .map(|part| part.into_iter().map(|i| self.identities[i]).collect())
            .collect()
    }
    pub fn cycles(&self) -> Vec<Vec<DefinitionId>> {
        self.strongly_connected_components()
            .into_iter()
            .filter(|group| {
                group.len() > 1 || {
                    let index = self.indices[&group[0]];
                    self.edges[index].contains(&index)
                }
            })
            .collect()
    }
}
