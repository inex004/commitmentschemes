#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use libp2p::{identity, PeerId};
#[cfg(target_arch = "wasm32")]
use std::sync::{Arc, Mutex};
#[cfg(target_arch = "wasm32")]
use rand::{thread_rng, Rng};

// We import your exact cryptography math!
#[cfg(target_arch = "wasm32")]
#[path = "crypto.rs"]
pub mod crypto;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BrowserNode {
    pub_key_str: String,
    peer_id_str: String,
    // We store the secret nonce in memory so the phone can use it later during the Reveal Phase
    secret_nonce: Arc<Mutex<Option<[u8; 32]>>>,
    secret_bid: Arc<Mutex<Option<u64>>>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BrowserNode {
    
    /// This is called exactly once when the web page loads on the phone.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        // This ensures Rust panics show up nicely in the Chrome/Safari console
        console_error_panic_hook::set_once();
        
        // 1. Generate a totally unique, secure identity for this specific phone!
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();
        
        Self {
            pub_key_str: hex::encode(local_key.public().encode_protobuf()),
            peer_id_str: local_peer_id.to_string(),
            secret_nonce: Arc::new(Mutex::new(None)),
            secret_bid: Arc::new(Mutex::new(None)),
        }
    }

    /// The webpage calls this to get the phone's unique PeerID to display on screen
    #[wasm_bindgen]
    pub fn get_peer_id(&self) -> String {
        self.peer_id_str.clone()
    }

    /// The webpage calls this when the user taps "Submit Bid".
    /// It runs your heavy cryptographic math natively on the phone's CPU!
    #[wasm_bindgen]
    pub fn generate_commit_payload(&self, bid_amount: u64) -> String {
        // 1. Generate 256 bits of pure entropy on the phone
        let mut rng = thread_rng();
        let mut nonce = [0u8; 32];
        rng.fill(&mut nonce);
        
        // Save the secrets in the phone's RAM for the reveal phase
        *self.secret_bid.lock().unwrap() = Some(bid_amount);
        *self.secret_nonce.lock().unwrap() = Some(nonce);
        
        // 2. Hash 1: Derive the Identity-Bound Scalar
        let s = crypto::derive_scalar(&self.peer_id_str, &nonce);
        
        // 3. Curve Math: Calculate the Pedersen Commitment
        let my_commitment = crypto::commit(bid_amount, s);
        
        // 4. Hash 2: Calculate the opaque payload hash
        let payload_hash = crypto::generate_payload_hash(my_commitment);
        
        // Return this to the Javascript so it can be gossiped!
        payload_hash
    }

    /// The webpage calls this when the Reveal Phase starts
    #[wasm_bindgen]
    pub fn get_reveal_nonce_hex(&self) -> String {
        if let Some(nonce) = *self.secret_nonce.lock().unwrap() {
            hex::encode(nonce)
        } else {
            String::from("ERROR_NO_NONCE")
        }
    }
}