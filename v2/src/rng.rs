//! A small source of pseudo-random numbers (SplitMix64).
//!
//! The generator of programs, the seeded host and the property tests draw
//! from it. One seed gives one sequence on every machine, so a failing case
//! is named by its seed.

#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    pub fn word(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    /// A number below `bound`, which is not zero.
    pub fn below(&mut self, bound: u64) -> u64 {
        self.word() % bound
    }

    pub fn index(&mut self, length: usize) -> usize {
        self.below(length as u64) as usize
    }

    /// A number from `low` to `high`, both included. The range is at most
    /// `2^127` wide.
    pub fn range(&mut self, low: i128, high: i128) -> i128 {
        let width = (high - low) as u128 + 1;
        let drawn = (u128::from(self.word()) << 64) | u128::from(self.word());
        low + (drawn % width) as i128
    }

    /// True in `numerator` cases of `denominator`.
    pub fn chance(&mut self, numerator: u64, denominator: u64) -> bool {
        self.below(denominator) < numerator
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.index(items.len())]
    }

    /// A source of its own, so that what one part draws does not shift what
    /// the next part gets.
    pub fn fork(&mut self) -> Rng {
        Rng(self.word())
    }
}

/// A number that depends on the words alone: the answer of a host that has
/// no state.
pub fn mix(words: &[u64]) -> u64 {
    let mut rng = Rng(0x243f_6a88_85a3_08d3);
    for word in words {
        rng.0 ^= *word;
        rng.word();
    }
    rng.word()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_seed_gives_one_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        let first: Vec<u64> = (0..8).map(|_| a.word()).collect();
        let second: Vec<u64> = (0..8).map(|_| b.word()).collect();
        assert_eq!(first, second);
        assert_ne!(first[0], first[1]);
        // The published first output of SplitMix64 for the seed 0.
        assert_eq!(Rng::new(0).word(), 0xe220_a839_7b1d_cdaf);
    }

    #[test]
    fn a_range_keeps_its_ends() {
        let mut rng = Rng::new(1);
        let mut seen = [false; 5];
        for _ in 0..200 {
            let value = rng.range(-2, 2);
            assert!((-2..=2).contains(&value));
            seen[(value + 2) as usize] = true;
        }
        assert!(seen.iter().all(|hit| *hit));
        assert_eq!(rng.range(9, 9), 9);
        let wide = rng.range(i128::from(i64::MIN), i128::from(u64::MAX));
        assert!(wide >= i128::from(i64::MIN) && wide <= i128::from(u64::MAX));
    }

    #[test]
    fn mixing_depends_on_every_word() {
        assert_eq!(mix(&[1, 2, 3]), mix(&[1, 2, 3]));
        assert_ne!(mix(&[1, 2, 3]), mix(&[1, 2, 4]));
        assert_ne!(mix(&[1, 2, 3]), mix(&[2, 1, 3]));
    }
}
