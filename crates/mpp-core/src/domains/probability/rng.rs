//! A small, self-contained, seeded pseudo-random generator so sampling is reproducible
//! (`%seed 42` gives the same numbers on every platform and every run).
//!
//! **Algorithm (fixed; changing it changes every seeded result):**
//! - Seeding: the 64-bit seed is expanded into the 256-bit state by four steps of
//!   SplitMix64 (Steele, Lea & Flood 2014): `z += 0x9E3779B97F4A7C15;
//!   z = (z ^ (z >> 30)) · 0xBF58476D1CE4E5B9; z = (z ^ (z >> 27)) · 0x94D049BB133111EB;
//!   z ^ (z >> 31)`.
//! - Generator: xoshiro256** (Blackman & Vigna 2018): output `rotl(s₁ · 5, 7) · 9`.
//! - Uniform on [0, 1): the top 53 bits, `(x >> 11) · 2⁻⁵³` (exact in every IEEE double).
//! - Normal: Box–Muller from two uniforms, `√(−2 ln(1 − u₁)) cos(2π u₂)` (the uniform
//!   bits are bit-exact everywhere; `ln` and `cos` come from the platform's maths
//!   library, which can differ in the last bit).
//! - Integers below n: Lemire's multiply-and-reject method (unbiased).

use std::f64::consts::PI;

/// The generator state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix(z: &mut u64) -> u64 {
    *z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut x = *z;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Rng {
    /// A generator seeded from a 64-bit integer.
    pub fn seeded(seed: u64) -> Rng {
        let mut z = seed;
        let s = [
            splitmix(&mut z),
            splitmix(&mut z),
            splitmix(&mut z),
            splitmix(&mut z),
        ];
        Rng { s }
    }

    /// A generator with an explicit state (must not be all zero).
    pub fn from_state(s: [u64; 4]) -> Rng {
        if s == [0; 4] {
            return Rng::seeded(0);
        }
        Rng { s }
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// Uniform on [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform on [a, b).
    pub fn uniform_in(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.uniform()
    }

    /// Standard normal (Box–Muller, one value per call).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.uniform();
        let u2 = self.uniform();
        (-2.0 * (1.0 - u1).ln()).sqrt() * (2.0 * PI * u2).cos()
    }

    /// Uniform integer in 0 … n−1 (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        let n = n.max(1);
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = self.next_u64() as u128 * n as u128;
            if (m as u64) >= threshold {
                return (m >> 64) as u64;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_vectors() {
        // xoshiro256** from state (1, 2, 3, 4): rotl(2·5, 7)·9 = 11520, then 0
        // (worked by hand from Blackman & Vigna's reference code).
        let mut r = Rng::from_state([1, 2, 3, 4]);
        assert_eq!(r.next_u64(), 11520);
        assert_eq!(r.next_u64(), 0);
        // SplitMix64 from seed 0 gives 0xE220A8397B1DCDAF first (Vigna's splitmix64.c).
        let mut z = 0u64;
        assert_eq!(splitmix(&mut z), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn reproducible_and_in_range() {
        let a: Vec<u64> = {
            let mut r = Rng::seeded(42);
            (0..5).map(|_| r.next_u64()).collect()
        };
        let b: Vec<u64> = {
            let mut r = Rng::seeded(42);
            (0..5).map(|_| r.next_u64()).collect()
        };
        assert_eq!(a, b);
        assert_ne!(Rng::seeded(43).next_u64(), a[0]);
        let mut r = Rng::seeded(7);
        let n = 200_000;
        let (mut sum, mut sq) = (0.0, 0.0);
        for _ in 0..n {
            let u = r.uniform();
            assert!((0.0..1.0).contains(&u));
            sum += u;
            sq += u * u;
        }
        let mean = sum / n as f64;
        // Uniform[0, 1]: mean 1/2, variance 1/12 (catalogue); 5σ tolerances.
        assert!((mean - 0.5).abs() < 5.0 * (1.0 / 12.0 / n as f64).sqrt());
        assert!((sq / n as f64 - mean * mean - 1.0 / 12.0).abs() < 0.002);
        let normals: Vec<f64> = (0..n).map(|_| r.normal()).collect();
        let m = normals.iter().sum::<f64>() / n as f64;
        let v = normals.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / n as f64;
        assert!(m.abs() < 0.015 && (v - 1.0).abs() < 0.02);
        let counts = (0..60_000).fold([0u32; 6], |mut c, _| {
            c[r.below(6) as usize] += 1;
            c
        });
        assert!(counts.iter().all(|&c| (9_400..10_600).contains(&c)));
    }
}
