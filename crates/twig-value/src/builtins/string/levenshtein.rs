//! `levenshtein()` (reference/php/stdlib.md §5.10): Twig's "Did you mean" suggestions in
//! `SyntaxError::addSuggestions()` keep the alternatives with `levenshtein($name, $item) <=
//! strlen($name) / 3`.

use super::{long_param, str_param};
use crate::diag::Diagnostics;
use crate::error::PhpResult;
use crate::value::Value;

/// Port of `reference_levdist()` (PHPSRC/ext/standard/levenshtein.c): byte-wise edit distance with
/// the given insertion, replacement and deletion costs. The arithmetic wraps like the C `zend_long`
/// code does on x86-64.
pub fn levenshtein_bytes(s1: &[u8], s2: &[u8], cost_ins: i64, cost_rep: i64, cost_del: i64) -> i64 {
    if s1.is_empty() {
        return (s2.len() as i64).wrapping_mul(cost_ins);
    }
    if s2.is_empty() {
        return (s1.len() as i64).wrapping_mul(cost_del);
    }
    // With equal costs the distance is symmetric; the shorter string makes the rows.
    let (s1, s2) = if s1.len() < s2.len() && cost_ins == cost_rep && cost_rep == cost_del {
        (s2, s1)
    } else {
        (s1, s2)
    };
    let mut p1: Vec<i64> = (0..=s2.len())
        .map(|i| (i as i64).wrapping_mul(cost_ins))
        .collect();
    let mut p2: Vec<i64> = vec![0; s2.len() + 1];
    for &c1 in s1 {
        p2[0] = p1[0].wrapping_add(cost_del);
        for (i2, &c2) in s2.iter().enumerate() {
            let mut c0 = p1[i2].wrapping_add(if c1 == c2 { 0 } else { cost_rep });
            let c1v = p1[i2 + 1].wrapping_add(cost_del);
            if c1v < c0 {
                c0 = c1v;
            }
            let c2v = p2[i2].wrapping_add(cost_ins);
            if c2v < c0 {
                c0 = c2v;
            }
            p2[i2 + 1] = c0;
        }
        std::mem::swap(&mut p1, &mut p2);
    }
    p1[s2.len()]
}

/// Port of `PHP_FUNCTION(levenshtein)` (`levenshtein(string $string1, string $string2, int
/// $insertion_cost = 1, int $replacement_cost = 1, int $deletion_cost = 1): int`).
pub fn levenshtein(
    string1: &Value,
    string2: &Value,
    insertion_cost: Option<&Value>,
    replacement_cost: Option<&Value>,
    deletion_cost: Option<&Value>,
    diag: &mut dyn Diagnostics,
) -> PhpResult<i64> {
    const F: &str = "levenshtein";
    let s1 = str_param(string1, F, 1, "string1", diag)?;
    let s2 = str_param(string2, F, 2, "string2", diag)?;
    let mut cost = |v: Option<&Value>, pos: u32, name: &str| -> PhpResult<i64> {
        match v {
            Some(v) => long_param(v, F, pos, name, diag),
            None => Ok(1),
        }
    };
    let ins = cost(insertion_cost, 3, "insertion_cost")?;
    let rep = cost(replacement_cost, 4, "replacement_cost")?;
    let del = cost(deletion_cost, 5, "deletion_cost")?;
    Ok(levenshtein_bytes(&s1, &s2, ins, rep, del))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances() {
        assert_eq!(levenshtein_bytes(b"kitten", b"sitting", 1, 1, 1), 3);
        assert_eq!(levenshtein_bytes(b"", b"abc", 2, 1, 1), 6);
        assert_eq!(
            levenshtein_bytes(b"kitten", b"sitting", i64::MAX, 1, 1),
            -9223372036854775804
        );
    }
}
