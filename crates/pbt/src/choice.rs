//! Choice-sequence engine for property-based testing.
//!
//! Generators record their randomness as a sequence of `draw(max)` calls.
//! Shrinking operates on the choice sequence, not the generated values,
//! which makes it type-agnostic and composable.

/// SplitMix64 PRNG — fast, deterministic, good quality for test generation.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    /// Generate the next u64.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Draw a uniform integer in `[0, max]`.
    ///
    /// Returns 0 when `max == 0`.
    pub fn draw(&mut self, max: u64) -> u64 {
        if max == 0 {
            return 0;
        }
        // Rejection sampling to avoid modulo bias.
        let range = max.wrapping_add(1);
        if range == 0 {
            // max == u64::MAX → any value is valid
            return self.next_u64();
        }
        let limit = u64::MAX - (u64::MAX % range);
        loop {
            let val = self.next_u64();
            if val < limit {
                return val % range;
            }
        }
    }
}

/// A recorded sequence of choices (integers-in-ranges).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChoiceSequence {
    pub choices: Vec<u64>,
    pub maxima: Vec<u64>,
}

impl ChoiceSequence {
    pub fn new(choices: Vec<u64>, maxima: Vec<u64>) -> Self {
        debug_assert_eq!(choices.len(), maxima.len());
        ChoiceSequence { choices, maxima }
    }

    pub fn len(&self) -> usize {
        self.choices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.choices.is_empty()
    }
}

/// Trait for drawing choices — implemented by both recorder and replayer.
pub trait ChoiceSource {
    /// Draw a value in `[0, max]`. Returns `None` if exhausted (replayer).
    fn draw(&mut self, max: u64) -> Option<u64>;
}

/// Records choices from an RNG during generation.
pub struct ChoiceRecorder {
    rng: Rng,
    choices: Vec<u64>,
    maxima: Vec<u64>,
}

impl ChoiceRecorder {
    pub fn new(seed: u64) -> Self {
        ChoiceRecorder {
            rng: Rng::new(seed),
            choices: Vec::new(),
            maxima: Vec::new(),
        }
    }

    /// Consume the recorder and return the recorded sequence.
    pub fn into_sequence(self) -> ChoiceSequence {
        ChoiceSequence::new(self.choices, self.maxima)
    }
}

impl ChoiceSource for ChoiceRecorder {
    fn draw(&mut self, max: u64) -> Option<u64> {
        let val = self.rng.draw(max);
        self.choices.push(val);
        self.maxima.push(max);
        Some(val)
    }
}

/// Replays a previously recorded choice sequence (used during shrinking).
pub struct ChoiceReplayer {
    sequence: ChoiceSequence,
    cursor: usize,
}

impl ChoiceReplayer {
    pub fn new(sequence: ChoiceSequence) -> Self {
        ChoiceReplayer {
            sequence,
            cursor: 0,
        }
    }
}

impl ChoiceSource for ChoiceReplayer {
    fn draw(&mut self, max: u64) -> Option<u64> {
        if self.cursor >= self.sequence.choices.len() {
            return None;
        }
        let val = self.sequence.choices[self.cursor];
        // Clamp to the requested max (the shrunk value might exceed a
        // different max if the sequence was edited).
        let clamped = val.min(max);
        self.cursor += 1;
        Some(clamped)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn rng_deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn rng_draw_in_range() {
        let mut rng = Rng::new(123);
        for _ in 0..1000 {
            let val = rng.draw(10);
            assert!(val <= 10);
        }
    }

    #[test]
    fn rng_draw_zero_max() {
        let mut rng = Rng::new(99);
        assert_eq!(rng.draw(0), 0);
    }

    #[test]
    fn recorder_captures_choices() {
        let mut rec = ChoiceRecorder::new(42);
        let a = rec.draw(100).unwrap();
        let b = rec.draw(1).unwrap();
        let seq = rec.into_sequence();
        assert_eq!(seq.choices.len(), 2);
        assert_eq!(seq.choices[0], a);
        assert_eq!(seq.choices[1], b);
        assert_eq!(seq.maxima, vec![100, 1]);
    }

    #[test]
    fn replayer_reproduces_choices() {
        let mut rec = ChoiceRecorder::new(42);
        let a = rec.draw(100).unwrap();
        let b = rec.draw(50).unwrap();
        let seq = rec.into_sequence();

        let mut rep = ChoiceReplayer::new(seq);
        assert_eq!(rep.draw(100), Some(a));
        assert_eq!(rep.draw(50), Some(b));
        assert_eq!(rep.draw(10), None);
    }

    #[test]
    fn replayer_clamps_to_max() {
        let seq = ChoiceSequence::new(vec![100], vec![200]);
        let mut rep = ChoiceReplayer::new(seq);
        // If the replay is asked for max=50, clamp 100→50.
        assert_eq!(rep.draw(50), Some(50));
    }
}
