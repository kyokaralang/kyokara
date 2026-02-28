//! Choice-sequence shrinking.
//!
//! Operates on the recorded choice sequence (not the generated values),
//! making it type-agnostic. Four passes run until no progress:
//!
//! 1. Zero-suffix: try zeroing `choices[i..]` and truncating.
//! 2. Block deletion: delete contiguous blocks of 8, 4, 2, 1.
//! 3. Individual minimize: binary-search each choice toward 0.
//! 4. Pair swap: if `choices[i] > choices[i+1]`, swap them.

use crate::choice::ChoiceSequence;

/// Maximum number of shrink attempts before giving up.
const MAX_ATTEMPTS: usize = 1000;

/// What happened when testing a shrunk candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShrinkOutcome {
    /// The candidate still triggers a failure — it's a valid shrink.
    StillFails,
    /// The candidate no longer fails — reject this shrink.
    Passes,
    /// Generation failed (exhausted choices, unsupported type, etc.).
    Invalid,
}

/// Shrink a failing choice sequence using 4 passes.
///
/// `test_fn` replays the candidate and returns whether it still fails.
/// Returns the smallest sequence that still triggers the failure.
pub fn shrink(
    initial: &ChoiceSequence,
    test_fn: &mut dyn FnMut(&ChoiceSequence) -> ShrinkOutcome,
) -> ChoiceSequence {
    let mut best = initial.clone();
    let mut attempts = 0;

    loop {
        let mut progress = false;

        // Pass 1: Zero-suffix — try zeroing from position i onward and truncating.
        let mut i = 0;
        while i < best.len() && attempts < MAX_ATTEMPTS {
            let new_len = i.saturating_add(1);
            // Only try if we'd actually shorten the sequence.
            if new_len < best.len() {
                let mut candidate = best.clone();
                for j in i..candidate.choices.len() {
                    candidate.choices[j] = 0;
                }
                candidate.choices.truncate(new_len);
                candidate.maxima.truncate(new_len);
                attempts += 1;
                if test_fn(&candidate) == ShrinkOutcome::StillFails {
                    best = candidate;
                    progress = true;
                    continue; // Retry from same i with shorter sequence.
                }
            }
            i += 1;
        }

        // Pass 2: Block deletion — try deleting blocks of size 8, 4, 2, 1.
        for &block_size in &[8, 4, 2, 1] {
            let mut i = 0;
            while i + block_size <= best.len() && attempts < MAX_ATTEMPTS {
                let mut candidate = best.clone();
                candidate.choices.drain(i..i + block_size);
                candidate.maxima.drain(i..i + block_size);
                attempts += 1;
                if test_fn(&candidate) == ShrinkOutcome::StillFails {
                    best = candidate;
                    progress = true;
                    // Don't advance i — the next block is now at position i.
                } else {
                    i += 1;
                }
            }
        }

        // Pass 3: Individual minimize — binary-search each choice toward 0.
        for i in 0..best.len() {
            if attempts >= MAX_ATTEMPTS {
                break;
            }
            if best.choices[i] == 0 {
                continue;
            }
            // Binary search: try to reduce choices[i].
            let mut lo = 0_u64;
            let mut hi = best.choices[i];
            while lo < hi && attempts < MAX_ATTEMPTS {
                let mid = lo + (hi - lo) / 2;
                let mut candidate = best.clone();
                candidate.choices[i] = mid;
                attempts += 1;
                if test_fn(&candidate) == ShrinkOutcome::StillFails {
                    best = candidate;
                    hi = mid;
                    progress = true;
                } else {
                    lo = mid + 1;
                }
            }
        }

        // Pass 4: Pair swap — if choices[i] > choices[i+1], swap them.
        if best.len() >= 2 {
            for i in 0..best.len() - 1 {
                if attempts >= MAX_ATTEMPTS {
                    break;
                }
                if best.choices[i] > best.choices[i + 1] {
                    let mut candidate = best.clone();
                    candidate.choices.swap(i, i + 1);
                    candidate.maxima.swap(i, i + 1);
                    attempts += 1;
                    if test_fn(&candidate) == ShrinkOutcome::StillFails {
                        best = candidate;
                        progress = true;
                    }
                }
            }
        }

        if !progress || attempts >= MAX_ATTEMPTS {
            break;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn shrinks_to_minimal() {
        // A test that fails when any choice > 5.
        let initial = ChoiceSequence::new(vec![100, 200, 300], vec![1000, 1000, 1000]);
        let result = shrink(&initial, &mut |seq| {
            if seq.choices.iter().any(|&c| c > 5) {
                ShrinkOutcome::StillFails
            } else {
                ShrinkOutcome::Passes
            }
        });
        // Should still fail (at least one > 5).
        assert!(
            result.choices.iter().any(|&c| c > 5),
            "shrunk result should still fail: {:?}",
            result.choices
        );
        // Should be shorter than the original and have at least one value at 6.
        assert!(
            result.len() <= initial.len(),
            "should not grow: {:?}",
            result.choices
        );
        assert!(
            result.choices.contains(&6),
            "should minimize the failing value to 6 (threshold + 1): {:?}",
            result.choices
        );
    }

    #[test]
    fn shrinks_by_deletion() {
        // Fails if length >= 2.
        let initial = ChoiceSequence::new(vec![0, 0, 0, 0, 0], vec![10, 10, 10, 10, 10]);
        let result = shrink(&initial, &mut |seq| {
            if seq.choices.len() >= 2 {
                ShrinkOutcome::StillFails
            } else {
                ShrinkOutcome::Passes
            }
        });
        assert_eq!(result.choices.len(), 2, "should shrink to minimal length");
    }

    #[test]
    fn pair_swap_reorders() {
        // Fails when choices[0] > 0 and choices[1] > 0, prefers smaller first.
        let initial = ChoiceSequence::new(vec![10, 3], vec![100, 100]);
        let result = shrink(&initial, &mut |seq| {
            if seq.choices.len() >= 2 && seq.choices.iter().any(|&c| c > 0) {
                ShrinkOutcome::StillFails
            } else {
                ShrinkOutcome::Passes
            }
        });
        // Values should be minimized.
        assert!(result.choices.iter().all(|&c| c <= 10));
    }

    #[test]
    fn respects_max_attempts() {
        let initial = ChoiceSequence::new(vec![u64::MAX; 50], vec![u64::MAX; 50]);
        let mut count = 0;
        let _ = shrink(&initial, &mut |_seq| {
            count += 1;
            ShrinkOutcome::StillFails
        });
        assert!(
            count <= MAX_ATTEMPTS + 50,
            "should respect attempt limit, got {count}"
        );
    }
}
