use std::time::Instant;

use crate::schema::{ErrorInfo, PrimeIteration};

pub const SEGMENT_SIZE: usize = 32 * 1024;

pub fn known_pi(limit: u64) -> Option<u64> {
    match limit {
        10 => Some(4),
        100 => Some(25),
        1_000 => Some(168),
        10_000 => Some(1_229),
        100_000 => Some(9_592),
        1_000_000 => Some(78_498),
        10_000_000 => Some(664_579),
        100_000_000 => Some(5_761_455),
        1_000_000_000 => Some(50_847_534),
        _ => None,
    }
}

pub fn simple_sieve(limit: u64) -> Vec<u64> {
    if limit < 2 {
        return Vec::new();
    }
    let n = (limit + 1) as usize;
    let mut composite = vec![false; n];
    let mut primes = Vec::new();
    for i in 2..n {
        if !composite[i] {
            primes.push(i as u64);
            let mut j = i.saturating_mul(i);
            while j < n {
                composite[j] = true;
                j += i;
            }
        }
    }
    primes
}

pub fn segmented_sieve_count(limit: u64) -> u64 {
    if limit < 2 {
        return 0;
    }

    let sqrt_limit = (limit as f64).sqrt() as u64 + 1;
    let base_primes = simple_sieve(sqrt_limit);

    let mut count: u64 = 0;
    let mut buf = vec![false; SEGMENT_SIZE];
    let mut low: u64 = 2;

    while low <= limit {
        let high = (low + SEGMENT_SIZE as u64 - 1).min(limit);
        let span = (high - low + 1) as usize;
        for slot in buf[..span].iter_mut() {
            *slot = false;
        }

        for &p in &base_primes {
            if p * p > high {
                break;
            }
            let first = first_multiple_in_range(p, low);
            if first > high {
                continue;
            }
            let mut j = (first - low) as usize;
            let step = p as usize;
            while j < span {
                buf[j] = true;
                j += step;
            }
        }

        for slot in &buf[..span] {
            if !*slot {
                count += 1;
            }
        }

        low += SEGMENT_SIZE as u64;
    }

    count
}

fn first_multiple_in_range(p: u64, low: u64) -> u64 {
    let start = p * p;
    if start >= low {
        return start;
    }
    let rem = low % p;
    if rem == 0 {
        low
    } else {
        low + (p - rem)
    }
}

pub fn run_1t(limit: u64, iterations: u32) -> Result<Vec<PrimeIteration>, ErrorInfo> {
    let expected = known_pi(limit);
    let mut results = Vec::with_capacity(iterations as usize);

    for i in 0..iterations {
        let start = Instant::now();
        let prime_count = segmented_sieve_count(limit);
        let seconds = start.elapsed().as_secs_f64();

        if let Some(exp) = expected {
            if prime_count != exp {
                return Err(ErrorInfo {
                    kind: "correctness_check_failed".into(),
                    message: format!(
                        "prime count mismatch for limit {limit} on iteration {i}: expected {exp}, got {prime_count}"
                    ),
                });
            }
        }

        results.push(PrimeIteration { seconds, prime_count });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_sieve_handles_small_values() {
        assert!(simple_sieve(0).is_empty());
        assert!(simple_sieve(1).is_empty());
        assert_eq!(simple_sieve(2), vec![2]);
        assert_eq!(simple_sieve(10), vec![2, 3, 5, 7]);
    }

    #[test]
    fn segmented_sieve_matches_known_pi_values() {
        for &n in &[10u64, 100, 1_000, 10_000, 100_000, 1_000_000] {
            let expected = known_pi(n).unwrap();
            let got = segmented_sieve_count(n);
            assert_eq!(got, expected, "pi({n}) expected {expected}, got {got}");
        }
    }

    #[test]
    fn segmented_sieve_pi_10_million() {
        assert_eq!(segmented_sieve_count(10_000_000), 664_579);
    }

    #[test]
    fn run_1t_emits_one_iteration_per_request() {
        let r = run_1t(100_000, 3).unwrap();
        assert_eq!(r.len(), 3);
        for it in &r {
            assert_eq!(it.prime_count, 9_592);
            assert!(it.seconds >= 0.0);
        }
    }

    #[test]
    fn run_1t_reports_correctness_failure_for_unknown_limit_passes() {
        // limits without a known pi value should not fail validation
        let r = run_1t(12_345, 1).unwrap();
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn first_multiple_in_range_starts_at_p_squared_when_above_low() {
        assert_eq!(first_multiple_in_range(7, 10), 49);
        assert_eq!(first_multiple_in_range(3, 10), 12);
        assert_eq!(first_multiple_in_range(5, 25), 25);
    }
}
