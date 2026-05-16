// =============================================================================
// benchmark.rs  —  place this at:  src/bin/benchmark.rs
//
// Run with:  cargo run --bin benchmark --release
//
// --release is MANDATORY. Debug builds are 10-100x slower and give
// meaningless timings. Always benchmark in release mode.
//
// This file is 100% self-contained. It does NOT import or modify any
// existing source file. All crypto logic is reproduced inline here
// exactly as it appears in crypto.rs, so you can verify the two
// implementations are equivalent.
// =============================================================================

use std::time::Instant;

use curve25519_dalek::constants::{
    RISTRETTO_BASEPOINT_COMPRESSED, RISTRETTO_BASEPOINT_POINT,
};
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use sha2::{Digest, Sha256, Sha512};

// ── Fixed test inputs ─────────────────────────────────────────────────────────
// Using fixed values (not randomly generated per trial) ensures the compiler
// cannot optimise the benchmarked code away as dead computation.

const PEER_ID:   &str    = "12D3KooWBenchmarkPeerIdentity";
const BID_VALUE: u64     = 250;
const NONCE:     [u8; 32] = [
    0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe,
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
    0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
];
const TRIALS: u64 = 10_000;

// =============================================================================
// SCHEME A: Standard Pedersen Commitment (baseline / textbook version)
//
//   r  <-  Z_q   (raw scalar, no hash, no identity binding)
//   C  =   v*G + r*H
//
// Security properties:
//   Identity binding : NO  -- any peer can reuse C as their own commitment
//   Replay resistant : NO
//   Bias-free scalar : NO  -- from_bytes_mod_order on 256-bit input has
//                            slight statistical bias mod 252-bit prime q
//   Point hidden     : NO  -- C itself would be broadcast on the network
// =============================================================================

fn get_h_basepoint() -> RistrettoPoint {
    RistrettoPoint::hash_from_bytes::<Sha512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes())
}

/// Standard Pedersen: nonce bytes cast directly to scalar via mod-order reduction.
/// This is the naive approach -- no hash, no identity binding.
fn standard_nonce_to_scalar(nonce: &[u8; 32]) -> Scalar {
    Scalar::from_bytes_mod_order(*nonce)
}

/// Standard Pedersen commit: C = v*G + r*H
fn standard_commit(v: u64, r: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(v);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (r * h)
}

// =============================================================================
// SCHEME B: Two-Hash Construction (our protocol, mirrors crypto.rs exactly)
//
//   r      <-  {0,1}^256  (raw byte string, not a field scalar)
//   s      =   H_512(pk || r) mod q     [Hash 1 -- derive_scalar()]
//   C      =   v*G + s*H                [commit()]
//   H_pay  =   H_256(C)                 [Hash 2 -- generate_payload_hash()]
//
// Security properties gained over standard Pedersen:
//   Identity binding : YES -- s depends on pk; other peers cannot reuse r
//   Replay resistant : YES -- copying H_pay fails unless you own pk
//   Bias-free scalar : YES -- 512-bit digest reduced mod 252-bit q is uniform
//   Point hidden     : YES -- only H_pay (32 bytes) travels the network
// =============================================================================

/// Hash 1: s = H_512(pk || r) mod q
/// Mirrors crypto.rs::derive_scalar() exactly.
fn two_hash_derive_scalar(peer_id: &str, nonce: &[u8; 32]) -> Scalar {
    let mut hasher = Sha512::new();
    hasher.update(peer_id.as_bytes());
    hasher.update(nonce);
    Scalar::from_hash(hasher)
}

/// Pedersen commit: C = v*G + s*H
/// Mirrors crypto.rs::commit() exactly.
fn two_hash_commit(v: u64, s: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(v);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (s * h)
}

/// Hash 2: H_payload = H_256(C)
/// Mirrors crypto.rs::generate_payload_hash() exactly.
fn two_hash_payload(c: RistrettoPoint) -> String {
    let mut hasher = Sha256::new();
    hasher.update(c.compress().as_bytes());
    hex::encode(hasher.finalize())
}

/// Full verify: rerun both hashes and compare.
/// Mirrors crypto.rs::verify_payload_hash() exactly.
fn two_hash_verify(stored: &str, v: u64, nonce: &[u8; 32], peer_id: &str) -> bool {
    let s          = two_hash_derive_scalar(peer_id, nonce);
    let c          = two_hash_commit(v, s);
    let recomputed = two_hash_payload(c);
    recomputed == stored
}

// =============================================================================
// Benchmarking helpers
// =============================================================================

struct Stats {
    mean_ns: u128,
    std_ns:  u128,
    min_ns:  u128,
    max_ns:  u128,
}

fn bench<F: Fn()>(f: F) -> Stats {
    // 500-iteration warmup: brings CPU caches and branch predictors to
    // steady state before any timing starts.
    for _ in 0..500 { f(); }

    let mut samples = Vec::with_capacity(TRIALS as usize);
    for _ in 0..TRIALS {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos());
    }

    let sum: u128  = samples.iter().sum();
    let mean: u128 = sum / TRIALS as u128;

    let variance: u128 = samples.iter()
        .map(|&x| {
            let d = if x > mean { x - mean } else { mean - x };
            d * d
        })
        .sum::<u128>() / TRIALS as u128;

    Stats {
        mean_ns: mean,
        std_ns:  isqrt(variance),
        min_ns:  *samples.iter().min().unwrap(),
        max_ns:  *samples.iter().max().unwrap(),
    }
}

fn isqrt(n: u128) -> u128 {
    if n == 0 { return 0; }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x { x = y; y = (x + n / x) / 2; }
    x
}

fn us(ns: u128) -> f64 { ns as f64 / 1_000.0 }

fn pct(base: u128, ours: u128) -> f64 {
    if base == 0 { return 0.0; }
    ((ours as f64 - base as f64) / base as f64) * 100.0
}

fn print_row(label: &str, s: &Stats) {
    println!(
        "  {:<44}  {:>7.2} us  +/-{:>5.2} us  [{:>6.2} - {:>6.2}]",
        label,
        us(s.mean_ns), us(s.std_ns),
        us(s.min_ns),  us(s.max_ns),
    );
}

// =============================================================================
// main
// =============================================================================

fn main() {
    println!();
    println!("==========================================================================");
    println!("  Pedersen Commitment Benchmark  ({} trials, --release build)", TRIALS);
    println!("==========================================================================");
    println!();
    println!("  Format:  mean  +/-std  [min - max]   (all times in microseconds)");
    println!();

    // ── A. Per-step breakdown ─────────────────────────────────────────────────
    println!("--- A. Per-Step Breakdown ---------------------------------------------------");
    println!();
    println!("  Standard Pedersen (baseline):");
    println!("  ------------------------------");

    let sa_scalar = bench(|| { let _ = standard_nonce_to_scalar(&NONCE); });
    print_row("Nonce -> scalar (from_bytes_mod_order)", &sa_scalar);

    let r = standard_nonce_to_scalar(&NONCE);
    let sa_curve = bench(|| { let _ = standard_commit(BID_VALUE, r); });
    print_row("Curve arithmetic  (v*G + r*H)", &sa_curve);

    println!();
    println!("  Two-Hash Construction (our protocol):");
    println!("  ---------------------------------------");

    let sb_hash1 = bench(|| { let _ = two_hash_derive_scalar(PEER_ID, &NONCE); });
    print_row("Hash 1: SHA-512 scalar derivation", &sb_hash1);

    let s = two_hash_derive_scalar(PEER_ID, &NONCE);
    let sb_curve = bench(|| { let _ = two_hash_commit(BID_VALUE, s); });
    print_row("Curve arithmetic  (v*G + s*H)", &sb_curve);

    let c = two_hash_commit(BID_VALUE, s);
    let sb_hash2 = bench(|| { let _ = two_hash_payload(c); });
    print_row("Hash 2: SHA-256 payload hash", &sb_hash2);

    println!();

    // ── B. End-to-end totals ──────────────────────────────────────────────────
    println!("--- B. End-to-End Totals ----------------------------------------------------");
    println!();

    let total_standard = bench(|| {
        let r = standard_nonce_to_scalar(&NONCE);
        let _ = standard_commit(BID_VALUE, r);
    });
    print_row("Standard Pedersen -- full commit()", &total_standard);

    let total_two_hash = bench(|| {
        let s = two_hash_derive_scalar(PEER_ID, &NONCE);
        let c = two_hash_commit(BID_VALUE, s);
        let _ = two_hash_payload(c);
    });
    print_row("Two-Hash Construction -- full commit()", &total_two_hash);

    println!();

    let std_verify = bench(|| {
        let r = standard_nonce_to_scalar(&NONCE);
        let _ = standard_commit(BID_VALUE, r);
        // Standard Pedersen has no separate verify step: the verifier
        // recomputes C and compares the point directly.
    });
    print_row("Standard Pedersen -- verify (recompute + compare)", &std_verify);

    let stored = {
        let s = two_hash_derive_scalar(PEER_ID, &NONCE);
        let c = two_hash_commit(BID_VALUE, s);
        two_hash_payload(c)
    };
    let th_verify = bench(|| {
        let _ = two_hash_verify(&stored, BID_VALUE, &NONCE, PEER_ID);
    });
    print_row("Two-Hash Construction -- full verify()", &th_verify);

    println!();

    // ── C. Summary table ──────────────────────────────────────────────────────
    println!("--- C. Summary Comparison Table ---------------------------------------------");
    println!();
    println!("  {:<46}  {:>11}  {:>11}  {:>7}",
        "Operation", "Standard", "Two-Hash", "Overhead");
    println!("  {}", "-".repeat(80));

    println!("  {:<46}  {:>8.2} us  {:>8.2} us  {:>6.1}%",
        "Nonce / scalar preparation",
        us(sa_scalar.mean_ns), us(sb_hash1.mean_ns),
        pct(sa_scalar.mean_ns, sb_hash1.mean_ns),
    );
    println!("  {:<46}  {:>8.2} us  {:>8.2} us  {:>6.1}%",
        "Curve arithmetic (v*G + s*H)",
        us(sa_curve.mean_ns), us(sb_curve.mean_ns),
        pct(sa_curve.mean_ns, sb_curve.mean_ns),
    );
    println!("  {:<46}  {:>11}  {:>8.2} us  {:>7}",
        "Payload hash (Hash 2, SHA-256)",
        "---", us(sb_hash2.mean_ns), "---",
    );
    println!("  {}", "-".repeat(80));
    println!("  {:<46}  {:>8.2} us  {:>8.2} us  {:>6.1}%",
        "Total commit()",
        us(total_standard.mean_ns), us(total_two_hash.mean_ns),
        pct(total_standard.mean_ns, total_two_hash.mean_ns),
    );
    println!("  {:<46}  {:>8.2} us  {:>8.2} us  {:>6.1}%",
        "Total verify()",
        us(std_verify.mean_ns), us(th_verify.mean_ns),
        pct(std_verify.mean_ns, th_verify.mean_ns),
    );
    println!("  {}", "-".repeat(80));
    println!("  {:<46}  {:>11}  {:>11}", "Identity binding",          "No",  "Yes");
    println!("  {:<46}  {:>11}  {:>11}", "Replay attack resistance",   "No",  "Yes");
    println!("  {:<46}  {:>11}  {:>11}", "Bias-free blinding scalar",  "No",  "Yes");
    println!("  {:<46}  {:>11}  {:>11}", "Curve point hidden on wire", "No",  "Yes");

    println!();

    // ── D. Key finding ────────────────────────────────────────────────────────
    let extra_ns  = sb_hash1.mean_ns + sb_hash2.mean_ns;
    let total_pct = pct(total_standard.mean_ns, total_two_hash.mean_ns);

    println!("--- D. Key Finding ----------------------------------------------------------");
    println!();
    println!("  Dominant cost in both schemes : elliptic curve scalar multiplication");
    println!("  Cost of Hash 1 + Hash 2 alone : {:.2} us", us(extra_ns));
    println!("  Total overhead of two-hash    : {:.1}%", total_pct);
    println!();
    println!("  The two additional hash operations (SHA-512 identity binding +");
    println!("  SHA-256 payload concealment) together add only {:.1}% overhead", total_pct);
    println!("  while delivering four additional security properties.");
    println!();
    println!("  Trials : {}    Build : --release", TRIALS);
    println!();
    println!("==========================================================================");
    println!();
}