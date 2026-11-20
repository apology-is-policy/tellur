//! Mathematical integers.
//!
//! The core evaluates formulas over the mathematical integers ([E-Formula]),
//! and decides whether a result fits its type by comparing the mathematical
//! result with the bounds of the type ([E-Arith]). The oracle therefore keeps
//! every integer without a width. The representation is a sign and a
//! magnitude in base 2^32, least significant limb first, with no leading zero
//! limb. Zero is not negative.

use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct Int {
    negative: bool,
    magnitude: Vec<u32>,
}

fn trim(mut limbs: Vec<u32>) -> Vec<u32> {
    while limbs.last() == Some(&0) {
        limbs.pop();
    }
    limbs
}

fn compare(a: &[u32], b: &[u32]) -> Ordering {
    if a.len() != b.len() {
        return a.len().cmp(&b.len());
    }
    for index in (0..a.len()).rev() {
        if a[index] != b[index] {
            return a[index].cmp(&b[index]);
        }
    }
    Ordering::Equal
}

fn add(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(long.len() + 1);
    let mut carry = 0u64;
    for (index, limb) in long.iter().enumerate() {
        let sum = u64::from(*limb) + short.get(index).map_or(0, |v| u64::from(*v)) + carry;
        out.push(sum as u32);
        carry = sum >> 32;
    }
    if carry != 0 {
        out.push(carry as u32);
    }
    out
}

/// `a - b` for `a >= b`.
fn subtract(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(a.len());
    let mut borrow = 0i64;
    for (index, limb) in a.iter().enumerate() {
        let mut difference = i64::from(*limb) - borrow - b.get(index).map_or(0, |v| i64::from(*v));
        if difference < 0 {
            difference += 1 << 32;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out.push(difference as u32);
    }
    trim(out)
}

fn multiply(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, y) in b.iter().enumerate() {
            let term = u64::from(*x) * u64::from(*y) + u64::from(out[i + j]) + carry;
            out[i + j] = term as u32;
            carry = term >> 32;
        }
        out[i + b.len()] = carry as u32;
    }
    trim(out)
}

fn shift_left_one(limbs: &mut Vec<u32>, low: u32) {
    let mut carry = low;
    for limb in limbs.iter_mut() {
        let next = *limb >> 31;
        *limb = (*limb << 1) | carry;
        carry = next;
    }
    if carry != 0 {
        limbs.push(carry);
    }
}

/// Quotient and remainder of magnitudes, for a nonzero `b`. Binary long
/// division: slow and plain, which suits a definition.
fn divide(a: &[u32], b: &[u32]) -> (Vec<u32>, Vec<u32>) {
    if compare(a, b) == Ordering::Less {
        return (Vec::new(), a.to_vec());
    }
    let mut quotient = vec![0u32; a.len()];
    let mut remainder: Vec<u32> = Vec::new();
    for bit in (0..a.len() * 32).rev() {
        shift_left_one(&mut remainder, (a[bit / 32] >> (bit % 32)) & 1);
        if compare(&remainder, b) != Ordering::Less {
            remainder = subtract(&remainder, b);
            quotient[bit / 32] |= 1 << (bit % 32);
        }
    }
    (trim(quotient), remainder)
}

fn divide_small(limbs: &[u32], divisor: u32) -> (Vec<u32>, u32) {
    let mut out = vec![0u32; limbs.len()];
    let mut remainder = 0u64;
    for index in (0..limbs.len()).rev() {
        let current = (remainder << 32) | u64::from(limbs[index]);
        out[index] = (current / u64::from(divisor)) as u32;
        remainder = current % u64::from(divisor);
    }
    (trim(out), remainder as u32)
}

impl Int {
    fn build(negative: bool, magnitude: Vec<u32>) -> Int {
        let magnitude = trim(magnitude);
        Int {
            negative: negative && !magnitude.is_empty(),
            magnitude,
        }
    }

    pub fn zero() -> Int {
        Int::default()
    }

    pub fn from_u128(value: u128) -> Int {
        let limbs = (0..4).map(|index| (value >> (32 * index)) as u32).collect();
        Int::build(false, limbs)
    }

    pub fn from_i128(value: i128) -> Int {
        let mut out = Int::from_u128(value.unsigned_abs());
        out.negative = value < 0;
        out
    }

    pub fn from_u64(value: u64) -> Int {
        Int::from_u128(u128::from(value))
    }

    /// `2^bits`.
    pub fn power_of_two(bits: u32) -> Int {
        let mut limbs = vec![0u32; (bits / 32) as usize + 1];
        limbs[(bits / 32) as usize] = 1 << (bits % 32);
        Int::build(false, limbs)
    }

    pub fn to_i128(&self) -> Option<i128> {
        if self.magnitude.len() > 4 {
            return None;
        }
        let mut value = 0u128;
        for (index, limb) in self.magnitude.iter().enumerate() {
            value |= u128::from(*limb) << (32 * index);
        }
        if !self.negative {
            i128::try_from(value).ok()
        } else if value == 1u128 << 127 {
            Some(i128::MIN)
        } else {
            i128::try_from(value).ok().map(|v| -v)
        }
    }

    pub fn is_zero(&self) -> bool {
        self.magnitude.is_empty()
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }

    /// Decimal digits with an optional leading minus sign.
    pub fn parse(text: &str) -> Option<Int> {
        let (negative, digits) = match text.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        if digits.is_empty() {
            return None;
        }
        let mut magnitude: Vec<u32> = Vec::new();
        for character in digits.chars() {
            let digit = character.to_digit(10)?;
            magnitude = add(&multiply(&magnitude, &[10]), &[digit]);
            magnitude = trim(magnitude);
        }
        Some(Int::build(negative, magnitude))
    }

    pub fn negate(&self) -> Int {
        Int::build(!self.negative, self.magnitude.clone())
    }

    pub fn plus(&self, other: &Int) -> Int {
        if self.negative == other.negative {
            return Int::build(self.negative, add(&self.magnitude, &other.magnitude));
        }
        match compare(&self.magnitude, &other.magnitude) {
            Ordering::Equal => Int::zero(),
            Ordering::Greater => {
                Int::build(self.negative, subtract(&self.magnitude, &other.magnitude))
            }
            Ordering::Less => {
                Int::build(other.negative, subtract(&other.magnitude, &self.magnitude))
            }
        }
    }

    pub fn minus(&self, other: &Int) -> Int {
        self.plus(&other.negate())
    }

    pub fn times(&self, other: &Int) -> Int {
        Int::build(
            self.negative != other.negative,
            multiply(&self.magnitude, &other.magnitude),
        )
    }

    /// Quotient truncated toward zero, and the remainder with the sign of the
    /// dividend ([E-Arith]). `None` for a zero divisor.
    pub fn divide(&self, other: &Int) -> Option<(Int, Int)> {
        if other.is_zero() {
            return None;
        }
        let (quotient, remainder) = divide(&self.magnitude, &other.magnitude);
        Some((
            Int::build(self.negative != other.negative, quotient),
            Int::build(self.negative, remainder),
        ))
    }
}

impl Ord for Int {
    fn cmp(&self, other: &Int) -> Ordering {
        match (self.negative, other.negative) {
            (false, true) => Ordering::Greater,
            (true, false) => Ordering::Less,
            (false, false) => compare(&self.magnitude, &other.magnitude),
            (true, true) => compare(&other.magnitude, &self.magnitude),
        }
    }
}

impl PartialOrd for Int {
    fn partial_cmp(&self, other: &Int) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Int {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return formatter.write_str("0");
        }
        let mut groups = Vec::new();
        let mut rest = self.magnitude.clone();
        while !rest.is_empty() {
            let (quotient, remainder) = divide_small(&rest, 1_000_000_000);
            groups.push(remainder);
            rest = quotient;
        }
        let mut text = String::new();
        if self.negative {
            text.push('-');
        }
        text.push_str(&groups.pop().unwrap_or(0).to_string());
        while let Some(group) = groups.pop() {
            text.push_str(&format!("{group:09}"));
        }
        formatter.write_str(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::Int;

    /// A small generator, so that the test needs no dependency.
    fn next(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = *state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Values of every size up to 62 bits, of both signs, with the edges of
    /// the machine types among them. A product of two stays within `i128`.
    fn sample(state: &mut u64) -> i128 {
        let bits = 1 + next(state) % 62;
        let raw = (next(state) >> (64 - bits)) as i128;
        match next(state) % 8 {
            0 => 0,
            1 => i128::from(i64::MAX),
            2 => i128::from(i64::MIN),
            3 => i128::from(u32::MAX),
            4 => -raw,
            _ => raw,
        }
    }

    #[test]
    fn agrees_with_machine_arithmetic() {
        let mut state = 7;
        for _ in 0..20_000 {
            let (a, b) = (sample(&mut state), sample(&mut state));
            let (x, y) = (Int::from_i128(a), Int::from_i128(b));
            assert_eq!(x.plus(&y).to_i128(), Some(a + b), "{a} + {b}");
            assert_eq!(x.minus(&y).to_i128(), Some(a - b), "{a} - {b}");
            assert_eq!(x.times(&y).to_i128(), Some(a * b), "{a} * {b}");
            assert_eq!(x.cmp(&y), a.cmp(&b), "{a} ? {b}");
            match x.divide(&y) {
                None => assert_eq!(b, 0),
                Some((q, r)) => {
                    assert_eq!(q.to_i128(), Some(a / b), "{a} / {b}");
                    assert_eq!(r.to_i128(), Some(a % b), "{a} % {b}");
                }
            }
            assert_eq!(x.to_string(), a.to_string());
            assert_eq!(Int::parse(&a.to_string()), Some(x));
        }
    }

    #[test]
    fn leaves_the_machine_range() {
        let big = Int::power_of_two(64);
        assert_eq!(big.to_string(), "18446744073709551616");
        let square = big.times(&big);
        assert_eq!(square.to_i128(), None);
        assert_eq!(
            square.to_string(),
            "340282366920938463463374607431768211456"
        );
        let (quotient, remainder) = square.plus(&Int::from_u64(5)).divide(&big).unwrap();
        assert_eq!(quotient, big);
        assert_eq!(remainder, Int::from_u64(5));
        assert_eq!(
            Int::parse("-340282366920938463463374607431768211456"),
            Some(square.negate())
        );
        assert_eq!(Int::from_i128(i128::MIN).to_i128(), Some(i128::MIN));
        assert_eq!(Int::parse("-0"), Some(Int::zero()));
        assert_eq!(Int::parse("12a"), None);
    }
}
