//! Generator v1: FNV-1a-64 for labels, SplitMix64 for draws, Lemire's method
//! for bounded draws and Fisher–Yates for shuffles.
//!
//! All of it is a few lines of integer arithmetic with published golden values,
//! so the plugin, the browser, the CLI and Locora Cloud draw the same tasks. Do
//! not swap in `locora_api::journey::shuffle` or a crate: a change there would
//! silently change every daily task. Secrecy comes from seeds the server keeps,
//! never from this generator.

use crate::Seed;

/// FNV-1a-64 of `bytes` (offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// SplitMix64 with the seed as its initial state. One fresh generator per task.
#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: Seed) -> SplitMix64 {
        SplitMix64 { state: seed.get() }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A uniform draw from `0..n` (Lemire's multiply-and-shift with exact rejection).
    /// `n` must not be 0; for 0 it answers 0 without drawing again.
    pub fn below(&mut self, n: u64) -> u64 {
        let mut m = u128::from(self.next_u64()) * u128::from(n);
        let mut low = m as u64;
        if low < n {
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                m = u128::from(self.next_u64()) * u128::from(n);
                low = m as u64;
            }
        }
        (m >> 64) as u64
    }
}

/// Fisher–Yates from the end: for `i` from `len − 1` down to 1, swap `i` with `below(i + 1)`.
pub fn shuffle<T>(rng: &mut SplitMix64, items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_stays_in_range_and_handles_the_edges() {
        let mut rng = SplitMix64::new(Seed::masked(7));
        for n in [1u64, 2, 3, 7, 10, 52, u64::MAX] {
            for _ in 0..100 {
                assert!(rng.below(n) < n);
            }
        }
        assert_eq!(rng.below(0), 0);
    }

    #[test]
    fn below_rejects_exactly_the_biased_low_products() {
        // For n = 2^63 + 1 the threshold is 2^63 − 1, and the first two numbers
        // of seed 0 fall below it: the draw takes the third.
        let n = (1u64 << 63) + 1;
        let mut stream = SplitMix64::new(Seed::masked(0));
        let threshold = n.wrapping_neg() % n;
        let low = |x: u64| (u128::from(x) * u128::from(n)) as u64;
        let first = [stream.next_u64(), stream.next_u64(), stream.next_u64()];
        assert!(low(first[0]) < threshold && low(first[1]) < threshold);
        assert!(low(first[2]) >= threshold);
        let mut rng = SplitMix64::new(Seed::masked(0));
        let value = rng.below(n);
        assert_eq!(value, ((u128::from(first[2]) * u128::from(n)) >> 64) as u64);
        assert_eq!(value, 243_808_509_735_772_839);
        assert_eq!(rng.next_u64(), stream.next_u64());
    }

    #[test]
    fn shuffle_keeps_every_element() {
        let mut rng = SplitMix64::new(Seed::masked(1));
        let mut items: Vec<u8> = (0..52).collect();
        shuffle(&mut rng, &mut items);
        assert_ne!(items, (0..52).collect::<Vec<u8>>());
        items.sort_unstable();
        assert_eq!(items, (0..52).collect::<Vec<u8>>());
        // Nothing to shuffle in an empty or one-element slice, and no draw either.
        let mut empty: [u8; 0] = [];
        shuffle(&mut rng, &mut empty);
        let mut one = [9u8];
        shuffle(&mut rng, &mut one);
        assert_eq!(one, [9]);
    }
}
