use curve25519_dalek::constants::{RISTRETTO_BASEPOINT_POINT, RISTRETTO_BASEPOINT_COMPRESSED};
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use sha2::{Sha512, Digest}; // Sha256 has been completely removed!

pub fn get_h_basepoint() -> RistrettoPoint {
    RistrettoPoint::hash_from_bytes::<Sha512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes())
}

/// Hash 1: Derives the identity-bound blinding scalar: s = H_512(pk || r) mod q
pub fn derive_scalar(peer_id: &str, nonce: &[u8; 32]) -> Scalar {
    let mut hasher = Sha512::new();
    hasher.update(peer_id.as_bytes());
    hasher.update(nonce);
    Scalar::from_hash(hasher)
}

/// The raw Pedersen Commitment: C = v*G + s*H
pub fn commit(bid_value: u64, s: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(bid_value);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (s * h)
}

/// Verifies a reveal directly against the algebraic Ristretto Point (Zero-Knowledge ready)
pub fn verify_commitment(stored_commitment_hex: &str, bid: u64, nonce_hex: &str, peer_id: &str) -> bool {
    if let Ok(nonce_bytes) = hex::decode(nonce_hex) {
        if nonce_bytes.len() == 32 {
            let mut nonce = [0u8; 32];
            nonce.copy_from_slice(&nonce_bytes);
            
            // Step 1: Recompute identity-bound scalar
            let expected_s = derive_scalar(peer_id, &nonce);
            
            // Step 2: Recompute the raw algebraic commitment point
            let expected_commit = commit(bid, expected_s);
            
            // Step 3: Compress the curve point and compare it to the broadcasted hex
            let expected_hex = hex::encode(expected_commit.compress().as_bytes());
            
            return expected_hex == stored_commitment_hex;
        }
    }
    false
}