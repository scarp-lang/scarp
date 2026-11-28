//! Pack verified logical locals into physical stack lanes by CFG liveness.

use super::{definite_slots::SlotSet, ProgramOp, ScalarProgramIr};
use crate::scalar_ir::Operation;
use std::collections::VecDeque;

pub(in crate::scalar_ir) struct LocalStorage {
    pub(in crate::scalar_ir) physical_lanes: Vec<usize>,
    pub(in crate::scalar_ir) lane_count: usize,
}

struct Access {
    reads: SlotSet,
    writes: SlotSet,
    retired: SlotSet,
}

impl Access {
    fn for_step(op: &ProgramOp, group: &[usize]) -> Self {
        let mut reads = SlotSet::empty();
        let mut writes = SlotSet::empty();
        let mut retired = SlotSet::empty();
        if let Some(expression) = op.expression() {
            for instruction in &expression.steps {
                if let Operation::Load { slot, .. } = instruction.kind {
                    reads.insert(group[slot]);
                }
            }
        }
        match op {
            ProgramOp::Store { slot, .. } => writes.insert(group[*slot]),
            ProgramOp::Update { slot, .. } => {
                reads.insert(group[*slot]);
                writes.insert(group[*slot]);
            }
            ProgramOp::RepeatInit {
                count_slot,
                index_slot,
                ..
            } => {
                writes.insert(group[*count_slot]);
                writes.insert(group[*index_slot]);
            }
            ProgramOp::RepeatGuard {
                count_slot,
                index_slot,
                ..
            } => {
                reads.insert(group[*count_slot]);
                reads.insert(group[*index_slot]);
            }
            ProgramOp::RepeatNext { index_slot } => {
                reads.insert(group[*index_slot]);
                writes.insert(group[*index_slot]);
            }
            ProgramOp::Retire { slots } | ProgramOp::LoopControl { retire: slots, .. } => {
                for &slot in slots {
                    retired.insert(group[slot]);
                }
            }
            _ => {}
        }
        Self {
            reads,
            writes,
            retired,
        }
    }
}

impl LocalStorage {
    /// The caller has already verified local widths, initialization and
    /// branch targets. A composite remains one contiguous allocation.
    pub(in crate::scalar_ir) fn plan(program: &ScalarProgramIr) -> Self {
        let mut groups = Vec::with_capacity(program.slots.len());
        let mut widths = Vec::new();
        for width in program
            .slot_widths
            .iter()
            .copied()
            .filter(|width| *width != 0)
        {
            groups.resize(groups.len() + width, widths.len());
            widths.push(width);
        }
        let conflicts = interference(program, &groups, widths.len());
        let mut bases = Vec::with_capacity(widths.len());
        let mut lane_count = 0;
        for (group, &width) in widths.iter().enumerate() {
            let mut occupied = SlotSet::empty();
            for previous in conflicts[group].members().take_while(|id| *id < group) {
                let base = bases[previous];
                for lane in base..base + widths[previous] {
                    occupied.insert(lane);
                }
            }
            let base = (0..lane_count)
                .find(|&start| (start..start + width).all(|lane| !occupied.contains(lane)))
                .unwrap_or(lane_count);
            bases.push(base);
            lane_count = lane_count.max(base + width);
        }
        let physical_lanes = bases
            .into_iter()
            .zip(widths)
            .flat_map(|(base, width)| base..base + width)
            .collect();
        Self {
            physical_lanes,
            lane_count,
        }
    }
}

fn interference(program: &ScalarProgramIr, groups: &[usize], count: usize) -> Vec<SlotSet> {
    let len = program.steps.len();
    let mut predecessors = vec![Vec::new(); len + 1];
    let mut access = Vec::with_capacity(len);
    for (pc, step) in program.steps.iter().enumerate() {
        access.push(Access::for_step(&step.kind, groups));
        for target in step.kind.successors(pc).into_iter().flatten() {
            predecessors[target].push(pc);
        }
    }
    let mut live_in = vec![SlotSet::empty(); len + 1];
    let mut pending: VecDeque<_> = (0..len).rev().collect();
    let mut queued = vec![true; len];
    while let Some(pc) = pending.pop_front() {
        queued[pc] = false;
        let access = &access[pc];
        let incoming = live_after(program, &live_in, pc)
            .without(access.writes.union(access.retired))
            .union(access.reads);
        if incoming != live_in[pc] {
            live_in[pc] = incoming;
            for &predecessor in &predecessors[pc] {
                if !queued[predecessor] {
                    pending.push_back(predecessor);
                    queued[predecessor] = true;
                }
            }
        }
    }

    let mut conflicts = vec![SlotSet::empty(); count];
    let mut parameters = SlotSet::empty();
    for &group in &groups[..program.parameters()] {
        parameters.insert(group);
    }
    connect(&mut conflicts, parameters);
    for (pc, access) in access.iter().enumerate() {
        connect(&mut conflicts, live_in[pc]);
        // Even a dead destination still gets written by the encoder. It
        // cannot use a lane that a later instruction will need.
        connect(
            &mut conflicts,
            live_after(program, &live_in, pc).union(access.writes),
        );
    }
    conflicts
}

fn live_after(program: &ScalarProgramIr, live_in: &[SlotSet], pc: usize) -> SlotSet {
    program.steps[pc]
        .kind
        .successors(pc)
        .into_iter()
        .flatten()
        .fold(SlotSet::empty(), |live, target| live.union(live_in[target]))
}

fn connect(conflicts: &mut [SlotSet], simultaneously_used: SlotSet) {
    for member in simultaneously_used.members() {
        conflicts[member] = conflicts[member].union(simultaneously_used);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(source: &str) -> LocalStorage {
        let resolved = crate::compile(source).unwrap();
        let program = ScalarProgramIr::lower_bounded_integer_function(&resolved, 16).unwrap();
        LocalStorage::plan(&program)
    }

    #[test]
    fn mutually_exclusive_aggregates_share_contiguous_lanes() {
        let storage = plan(
            "pure funcra pick(index: integer) -> integer {
                if index == 0 {
                    bind left = [array: 10, 20, 30];
                    return left[index];
                }
                bind right = [array: 40, 50, 60];
                return right[index];
            }",
        );
        assert_eq!(storage.physical_lanes[0], 0);
        assert_eq!(&storage.physical_lanes[2..], [1, 2, 3, 1, 2, 3]);
        assert_eq!(storage.lane_count, 4);
    }

    #[test]
    fn loops_keep_live_locals_and_prevent_dead_stores_from_clobbering_them() {
        let storage = plan(
            "pure funcra loop_test(count: integer) -> integer {
                var total = 0;
                repeat count as i {
                    if i == 0 { continue; }
                    bind dead = 100;
                    total += i;
                }
                return total;
            }",
        );
        // The parameter dies when the repeat captures its value. The dead
        // write inside the loop still conflicts with the three loop locals.
        assert_eq!(storage.lane_count, 4);
        let mut active = storage.physical_lanes[1..].to_vec();
        active.sort_unstable();
        active.dedup();
        assert_eq!(active.len(), 4);
    }

    #[test]
    fn overlapping_snapshots_keep_distinct_lanes() {
        let storage = plan(
            "pure funcra snapshot(index: integer) -> integer {
                var values = [array: 1, 2];
                bind saved = values;
                values = [array: 3, 4];
                return saved[index] - values[index];
            }",
        );
        assert_eq!(storage.lane_count, 5);
    }
}
