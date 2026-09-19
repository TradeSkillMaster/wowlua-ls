//! Hash containers with a **deterministic** hasher.
//!
//! `std::collections::HashMap`'s default `RandomState` reseeds itself every
//! process start, so iteration order differs between runs of the same binary.
//! The engine leaks that order into results in a number of places — a union
//! built by walking a table's fields, the fixpoint worklist seeded from
//! `overlay_fields`, which of two writes to the same field resolves first — so
//! the *same* binary analysing the *same* sources could report a diagnostic on
//! one run and not the next, making before/after comparisons unreliable.
//!
//! These aliases pin the hasher to `rustc_hash::FxHasher`, whose output depends
//! only on the key bytes. Iteration order becomes a pure function of the keys
//! and the insertion sequence, so a run is reproducible. (Fx is also faster than
//! SipHash; it is not HashDoS-resistant, which is irrelevant for a language
//! server reading local files.)
//!
//! Prefer these over `std::collections::{HashMap, HashSet}` everywhere in the
//! workspace. The API is identical except for the constructors that `std` only
//! provides for `RandomState`:
//!
//! | std                        | here                                             |
//! |----------------------------|--------------------------------------------------|
//! | `HashMap::new()`           | `HashMap::default()`                             |
//! | `HashMap::from([..])`      | `HashMap::from_iter([..])`                       |
//! | `HashMap::with_capacity(n)`| `HashMap::with_capacity_and_hasher(n, FxBuildHasher)` |
//!
//! (and the same three for `HashSet`) — which is what [`FxBuildHasher`] is
//! re-exported for.

pub use rustc_hash::{FxBuildHasher, FxHashMap as HashMap, FxHashSet as HashSet};

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole determinism story rests on: two maps built
    /// independently — in the same process or across runs — iterate their keys
    /// in the same order. `std::collections::HashMap` fails this even within one
    /// process, because `RandomState::new()` advances a thread-local seed for
    /// every map it creates.
    #[test]
    fn iteration_order_is_independent_of_map_instance() {
        let keys: Vec<String> = (0..64).map(|i| format!("field{i}")).collect();
        let build = || {
            let mut m: HashMap<&str, usize> = HashMap::default();
            for (i, k) in keys.iter().enumerate() {
                m.insert(k.as_str(), i);
            }
            m.into_keys().collect::<Vec<_>>()
        };
        assert_eq!(build(), build());
    }
}
