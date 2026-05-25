// =============================================================================
// benchmark.rs  —  place this file at:  src/bin/benchmark.rs
//
// Run with:  cargo run --bin benchmark --release
//
// --release is MANDATORY. Debug builds are 10-100x slower and produce
// meaningless timings. Always benchmark in release mode.
//
// This file is 100% self-contained and does NOT import or modify any
// existing source file. All crypto logic is reproduced inline, mirroring
// the updated crypto.rs exactly.
//
// What changed vs. the old benchmark
// ------------------------------------
//   OLD:  compared Standard Pedersen vs. Two-Hash (SHA-512 + SHA-256)
//   NEW:  compares Standard Pedersen vs. Identity-Binding (SHA-512 only)
//
//   The SHA-256 payload-hash step (Hash 2) has been removed from the
//   protocol entirely. The 32-byte compressed Ristretto point is now
//   broadcast directly on the wire in both the commit and verify paths.
//   The benchmark reflects this: only one additional hash (SHA-512) is
//   timed and reported.
// =============================================================================

use std::time::Instant;

use curve25519_dalek::constants::{
    RISTRETTO_BASEPOINT_COMPRESSED,
    RISTRETTO_BASEPOINT_POINT,
};
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use sha2::{Digest, Sha512};

// ── Fixed test inputs ─────────────────────────────────────────────────────────
// Using fixed values (not randomly generated per trial) prevents the compiler
// from optimising away the benchmarked computation as dead code.

const PEER_ID:    &str     = "12D3KooWBenchmarkPeerIdentity";
const BID_VALUE:  u64      = 250;
const NONCE:      [u8; 32] = [
    0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe,
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef,
    0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10,
    0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
];
const TRIALS: u64 = 10_000;

// =============================================================================
//  SHARED: H basepoint (same in both schemes)
// =============================================================================

fn get_h_basepoint() -> RistrettoPoint {
    RistrettoPoint::hash_from_bytes::<Sha512>(
        RISTRETTO_BASEPOINT_COMPRESSED.as_bytes(),
    )
}

// =============================================================================
//  SCHEME A — Standard (Baseline) Pedersen Commitment
//
//  The nonce bytes are cast directly to a Ristretto scalar via a raw
//  mod-order reduction.  No hash, no identity information.
//
//      r  <-  {0,1}^256
//      s  =   from_bytes_mod_order(r)       // direct reduction, slight bias
//      C  =   v*G + s*H
//
//  Verification: recompute C, compare compressed points.
//
//  Security properties:
//      Identity binding   NO  — any peer can copy and reuse C
//      Replay resistant   NO
//      Bias-free scalar   NO  — 256-bit → 252-bit prime has residual bias
//      Algebraic on wire  YES — compressed C is broadcast
// =============================================================================

/// Cast nonce bytes to scalar via modular reduction (no hash).
fn baseline_nonce_to_scalar(nonce: &[u8; 32]) -> Scalar {
    Scalar::from_bytes_mod_order(*nonce)
}

/// C = v*G + s*H  (baseline, s is the raw-reduced nonce).
fn baseline_commit(v: u64, s: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(v);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (s * h)
}

/// Baseline verify: recompute C and compare compressed hex.
fn baseline_verify(stored_hex: &str, v: u64, nonce: &[u8; 32]) -> bool {
    let s   = baseline_nonce_to_scalar(nonce);
    let c   = baseline_commit(v, s);
    let hex = hex::encode(c.compress().as_bytes());
    hex == stored_hex
}

// =============================================================================
//  SCHEME B — Identity-Binding Pedersen Commitment  (our protocol)
//
//  Mirrors crypto.rs exactly after the single-hash upgrade.
//
//      r  <-  {0,1}^256                     // raw byte string
//      s  =   H_512(pk || r) mod q          // derive_scalar()
//      C  =   v*G + s*H                     // commit()
//      broadcast: compress(C)  [32 bytes]   // no further hashing
//
//  Verification: recompute s and C, compare compressed points directly.
//  Mirrors crypto.rs::verify_commitment() exactly.
//
//  Security properties gained over baseline:
//      Identity binding   YES — s depends on pk; copying C fails at reveal
//      Replay resistant   YES — SHA-512 collision required to forge
//      Bias-free scalar   YES — 512-bit digest reduced mod 252-bit q
//      Algebraic on wire  YES — compressed C broadcast, structure preserved
// =============================================================================

/// s = H_512(pk || r) mod q  — mirrors crypto.rs::derive_scalar().
fn identity_derive_scalar(peer_id: &str, nonce: &[u8; 32]) -> Scalar {
    let mut hasher = Sha512::new();
    hasher.update(peer_id.as_bytes());
    hasher.update(nonce);
    Scalar::from_hash(hasher)
}

/// C = v*G + s*H  — mirrors crypto.rs::commit().
fn identity_commit(v: u64, s: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(v);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (s * h)
}

/// Verify by recomputing C and comparing hex of compressed point.
/// Mirrors crypto.rs::verify_commitment().
fn identity_verify(stored_hex: &str, v: u64, nonce: &[u8; 32], peer_id: &str) -> bool {
    let s   = identity_derive_scalar(peer_id, nonce);
    let c   = identity_commit(v, s);
    let hex = hex::encode(c.compress().as_bytes());
    hex == stored_hex
}

// =============================================================================
//  Benchmarking helpers
// =============================================================================

struct Stats {
    mean_ns: u128,
    std_ns:  u128,
    min_ns:  u128,
    max_ns:  u128,
}

fn bench<F: Fn()>(f: F) -> Stats {
    // Warmup: brings CPU caches and branch predictors to steady state
    // before any timing begins.
    for _ in 0..500 { f(); }

    let mut samples = Vec::with_capacity(TRIALS as usize);
    for _ in 0..TRIALS {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_nanos());
    }

    let sum: u128  = samples.iter().sum();
    let mean: u128 = sum / TRIALS as u128;

    let variance: u128 = samples
        .iter()
        .map(|&x| {
            let d = if x > mean { x - mean } else { mean - x };
            d * d
        })
        .sum::<u128>()
        / TRIALS as u128;

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

fn us(ns: u128)              -> f64 { ns as f64 / 1_000.0 }
fn pct(base: u128, new: u128) -> f64 {
    if base == 0 { return 0.0; }
    ((new as f64 - base as f64) / base as f64) * 100.0
}

fn print_row(label: &str, s: &Stats) {
    println!(
        "  {:<46}  {:>7.2} us  +/-{:>5.2} us  [{:>6.2} .. {:>6.2}]",
        label,
        us(s.mean_ns), us(s.std_ns),
        us(s.min_ns),  us(s.max_ns),
    );
}

// =============================================================================
//  main
// =============================================================================

fn main() {
    println!();
    println!("==========================================================================");
    println!("  Identity-Binding Pedersen Benchmark  ({} trials, --release)", TRIALS);
    println!("==========================================================================");
    println!();
    println!("  Format:  mean  +/-std  [min .. max]   (all times in microseconds)");
    println!();

    // ── A. Per-step breakdown ─────────────────────────────────────────────────
    println!("--- A. Per-Step Breakdown ---------------------------------------------------");
    println!();
    println!("  Baseline Pedersen (standard, no identity binding):");
    println!("  ----------------------------------------------------");

    let sa_scalar = bench(|| { let _ = baseline_nonce_to_scalar(&NONCE); });
    print_row("Nonce → scalar (from_bytes_mod_order)", &sa_scalar);

    let s_base = baseline_nonce_to_scalar(&NONCE);
    let sa_curve = bench(|| { let _ = baseline_commit(BID_VALUE, s_base); });
    print_row("Curve arithmetic  (v·G + s·H)", &sa_curve);

    println!();
    println!("  Identity-Binding Pedersen (our protocol, SHA-512 scalar derivation):");
    println!("  -----------------------------------------------------------------------");

    let sb_hash1 = bench(|| { let _ = identity_derive_scalar(PEER_ID, &NONCE); });
    print_row("SHA-512 identity binding  (H₅₁₂(pk ‖ r) mod q)", &sb_hash1);

    let s_ours = identity_derive_scalar(PEER_ID, &NONCE);
    let sb_curve = bench(|| { let _ = identity_commit(BID_VALUE, s_ours); });
    print_row("Curve arithmetic  (v·G + s·H)", &sb_curve);

    println!();
    println!("  Note: no payload-hash step exists in the updated protocol.");
    println!("  The 32-byte compressed Ristretto point is broadcast directly.");
    println!();

    // ── B. End-to-end totals ──────────────────────────────────────────────────
    println!("--- B. End-to-End Totals ----------------------------------------------------");
    println!();

    let total_baseline = bench(|| {
        let s = baseline_nonce_to_scalar(&NONCE);
        let _ = baseline_commit(BID_VALUE, s);
    });
    print_row("Baseline Pedersen  — full commit()", &total_baseline);

    let total_ours = bench(|| {
        let s = identity_derive_scalar(PEER_ID, &NONCE);
        let _ = identity_commit(BID_VALUE, s);
    });
    print_row("Identity-Binding   — full commit()", &total_ours);

    println!();

    // Baseline verify: recompute C, compare compressed hex
    let stored_baseline = {
        let s = baseline_nonce_to_scalar(&NONCE);
        let c = baseline_commit(BID_VALUE, s);
        hex::encode(c.compress().as_bytes())
    };
    let baseline_verify_stat = bench(|| {
        let _ = baseline_verify(&stored_baseline, BID_VALUE, &NONCE);
    });
    print_row("Baseline Pedersen  — verify() (recompute + compare)", &baseline_verify_stat);

    // Identity-binding verify: recompute s, C, compare compressed hex
    let stored_ours = {
        let s = identity_derive_scalar(PEER_ID, &NONCE);
        let c = identity_commit(BID_VALUE, s);
        hex::encode(c.compress().as_bytes())
    };
    let ours_verify_stat = bench(|| {
        let _ = identity_verify(&stored_ours, BID_VALUE, &NONCE, PEER_ID);
    });
    print_row("Identity-Binding   — verify() (recompute + compare)", &ours_verify_stat);

    println!();

    // ── C. Summary comparison table ───────────────────────────────────────────
    println!("--- C. Summary Comparison Table ---------------------------------------------");
    println!();
    println!("  {:<48}  {:>11}  {:>11}  {:>8}",
             "Metric", "Baseline", "Ours", "Overhead");
    println!("  {}", "-".repeat(84));

    println!("  {:<48}  {:>8.2} us  {:>8.2} us  {:>7.1}%",
        "Nonce / scalar preparation",
        us(sa_scalar.mean_ns), us(sb_hash1.mean_ns),
        pct(sa_scalar.mean_ns, sb_hash1.mean_ns),
    );
    println!("  {:<48}  {:>8.2} us  {:>8.2} us  {:>7.1}%",
        "Curve arithmetic (v·G + s·H)",
        us(sa_curve.mean_ns), us(sb_curve.mean_ns),
        pct(sa_curve.mean_ns, sb_curve.mean_ns),
    );
    println!("  {}", "-".repeat(84));
    println!("  {:<48}  {:>8.2} us  {:>8.2} us  {:>7.1}%",
        "Total commit()",
        us(total_baseline.mean_ns), us(total_ours.mean_ns),
        pct(total_baseline.mean_ns, total_ours.mean_ns),
    );
    println!("  {:<48}  {:>8.2} us  {:>8.2} us  {:>7.1}%",
        "Total verify()",
        us(baseline_verify_stat.mean_ns), us(ours_verify_stat.mean_ns),
        pct(baseline_verify_stat.mean_ns, ours_verify_stat.mean_ns),
    );
    println!("  {}", "-".repeat(84));
    println!("  {:<48}  {:>11}  {:>11}", "Identity binding",         "No",  "Yes");
    println!("  {:<48}  {:>11}  {:>11}", "Replay attack resistance", "No",  "Yes");
    println!("  {:<48}  {:>11}  {:>11}", "Bias-free blinding scalar","No",  "Yes");
    println!("  {:<48}  {:>11}  {:>11}", "Algebraic structure on wire","Yes","Yes");

    println!();

    // ── D. Machine-readable CSV for plot_benchmark.py ────────────────────────
    println!("--- D. CSV Output (paste into plot_benchmark.py if needed) -----------------");
    println!();
    println!("step,baseline_us,ours_us");
    println!("Nonce/scalar prep,{:.4},{:.4}",
        us(sa_scalar.mean_ns), us(sb_hash1.mean_ns));
    println!("Curve arithmetic,{:.4},{:.4}",
        us(sa_curve.mean_ns), us(sb_curve.mean_ns));
    println!("Total commit(),{:.4},{:.4}",
        us(total_baseline.mean_ns), us(total_ours.mean_ns));
    println!("Total verify(),{:.4},{:.4}",
        us(baseline_verify_stat.mean_ns), us(ours_verify_stat.mean_ns));

    println!();

    // ── E. Key finding ────────────────────────────────────────────────────────
    let total_pct   = pct(total_baseline.mean_ns, total_ours.mean_ns);
    let verify_pct  = pct(baseline_verify_stat.mean_ns, ours_verify_stat.mean_ns);
    let hash_cost   = us(sb_hash1.mean_ns);

    println!("--- E. Key Finding ----------------------------------------------------------");
    println!();
    println!("  Dominant cost in both schemes : elliptic curve scalar multiplication");
    println!("  Cost of SHA-512 identity step : {:.2} us", hash_cost);
    println!("  Overhead on commit()          : {:.1}%", total_pct);
    println!("  Overhead on verify()          : {:.1}%", verify_pct);
    println!();
    println!("  The single SHA-512 identity-binding step adds only {:.1}% overhead",
             total_pct);
    println!("  while delivering three additional security properties:");
    println!("  identity binding, replay resistance, and a bias-free blinding scalar.");
    println!();
    println!("  Trials : {}    Build : --release", TRIALS);
    println!();
    println!("==========================================================================");
    println!();
}