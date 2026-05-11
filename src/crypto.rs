use curve25519_dalek::constants::{RISTRETTO_BASEPOINT_POINT, RISTRETTO_BASEPOINT_COMPRESSED};
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
// 🔥 FIX 1: Imported Sha512 for curve derivation, keeping Sha256 for the binding hash
use sha2::{Sha256, Sha512, Digest}; 

pub fn get_h_basepoint() -> RistrettoPoint {
    // 🔥 FIX 2: Using Sha512 to securely hash bytes directly onto the curve
    RistrettoPoint::hash_from_bytes::<Sha512>(RISTRETTO_BASEPOINT_COMPRESSED.as_bytes())
}

pub fn commit(bid_value: u64, blinding_factor: Scalar) -> RistrettoPoint {
    let v_scalar = Scalar::from(bid_value);
    let h = get_h_basepoint();
    (v_scalar * RISTRETTO_BASEPOINT_POINT) + (blinding_factor * h)
}

pub fn generate_binding_hash(commitment_hex: &str, peer_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(commitment_hex.as_bytes());
    hasher.update(peer_id.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn verify_binding_hash(stored_binding_hash: &str, bid: u64, blind_hex: &str, peer_id: &str) -> bool {
    if let Ok(blind_bytes) = hex::decode(blind_hex) {
        let mut bytes_array = [0u8; 32];
        bytes_array.copy_from_slice(&blind_bytes);
        
        // 🔥 FIX 3: Using `.into()` to safely convert the Constant-Time option to a standard Option
        let opt_blinding: Option<Scalar> = Scalar::from_canonical_bytes(bytes_array).into();
        
        if let Some(blinding_factor) = opt_blinding {
            let expected_commitment = commit(bid, blinding_factor);
            let expected_commit_hex = hex::encode(expected_commitment.compress().as_bytes());
            let expected_binding_hash = generate_binding_hash(&expected_commit_hex, peer_id);
            
            return expected_binding_hash == stored_binding_hash;
        }
    }
    false
}