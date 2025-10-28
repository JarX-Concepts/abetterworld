use std::collections::HashMap;
// or: use rustc_hash::FxHashMap as HashMap;

pub type TilePipelineState = HashMap<u64, usize>; // id -> child_count

#[inline]
pub fn has_tile(state: &TilePipelineState, id: u64) -> bool {
    state.contains_key(&id)
}

#[inline]
pub fn add_tile(state: &mut TilePipelineState, id: u64, child_count: usize) {
    state.insert(id, child_count);
}

#[inline]
pub fn child_count(state: &TilePipelineState, id: u64) -> Option<&usize> {
    state.get(&id)
}

#[inline]
pub fn upsert_child_count(state: &mut TilePipelineState, id: u64, count: usize) {
    state.insert(id, count); // updates if present
}

#[inline]
pub fn remove_tile(state: &mut TilePipelineState, id: u64) -> Option<usize> {
    state.remove(&id)
}
