use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use rand::Rng;
use sha2::{Sha256, Digest}; 

pub struct Auction {
    pub auction_id: String,
    pub seller_id: String,
    pub token_id: u64,
    pub reserve_price: u64,
    pub commit_deadline: u64,
    pub reveal_deadline: u64,
    pub received_commitments: HashMap<String, String>,
    pub verified_bids: HashMap<String, u64>,
    pub verified_nonces: HashMap<String, [u8; 32]>, // ✅ Updated state
    pub resolved: bool,
    pub failed: bool,
    pub winner_id: Option<String>,
    pub clearing_price: u64,
    pub slash_list: Vec<String>,

    pub validator_id: Option<String>,
    pub verdict_received: bool,
}

pub struct MarketplaceState {
    pub my_credits: u64,
    pub my_locked_credits: u64,
    pub my_art_vault: Vec<u64>,
    pub escrowed_art: Vec<u64>,
    pub active_auctions: HashMap<String, Auction>,
    pub current_joined_auction: Option<String>,
    pub my_secret_bid: Option<u64>,
    pub my_secret_nonce: Option<[u8; 32]>, // ✅ Updated state
}

impl MarketplaceState {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let starting_art = vec![
            rng.gen_range(100000..=999999),
            rng.gen_range(100000..=999999),
            rng.gen_range(100000..=999999),
        ];

        Self {
            my_credits: 1000,
            my_locked_credits: 0,
            my_art_vault: starting_art,
            escrowed_art: Vec::new(),
            active_auctions: HashMap::new(),
            current_joined_auction: None,
            my_secret_bid: None,
            my_secret_nonce: None, // ✅ Updated state
        }
    }
}

impl Auction {
    pub fn new(auction_id: String, seller_id: String, token_id: u64, reserve_price: u64) -> Self {
        let parts: Vec<&str> = auction_id.split('_').collect();
        let start_time = if parts.len() == 2 {
            parts[1].parse::<u64>().unwrap_or_else(|_| {
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
            })
        } else {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        };

        Self {
            auction_id,
            seller_id,
            token_id,
            reserve_price,
            commit_deadline: start_time + 240,
            reveal_deadline: start_time + 300,
            received_commitments: HashMap::new(),
            verified_bids: HashMap::new(),
            verified_nonces: HashMap::new(),
            resolved: false,
            failed: false,
            winner_id: None,
            clearing_price: 0,
            slash_list: Vec::new(),
            validator_id: None,
            verdict_received: false,
        }
    }

    pub fn resolve(&mut self) {
        if self.resolved { return; }
        self.resolved = true;

        for peer in self.received_commitments.keys() {
            if !self.verified_bids.contains_key(peer) {
                self.slash_list.push(peer.clone());
            }
        }

        let mut valid_bidders: Vec<(&String, &u64)> = self.verified_bids
            .iter()
            .filter(|(_, &bid)| bid >= self.reserve_price)
            .collect();

        if valid_bidders.is_empty() {
            self.failed = true;
            return;
        }

        valid_bidders.sort_by(|a, b| b.1.cmp(a.1));
        let highest_bid = *valid_bidders[0].1;

        let tied_bidders: Vec<&String> = valid_bidders
            .iter()
            .filter(|&&(_, bid)| *bid == highest_bid)
            .map(|&(id, _)| id)
            .collect();

        if tied_bidders.len() == 1 {
            self.winner_id = Some(tied_bidders[0].clone());
            self.clearing_price = if valid_bidders.len() > 1 {
                *valid_bidders[1].1
            } else {
                self.reserve_price
            };
        } else {
            println!("⚖️  TIE DETECTED! Executing cryptographic XOR tie-breaker...");

            // Step 1: XOR all tied bidders' 32-byte nonces to form S_tie
            let mut combined_xor = [0u8; 32];
            for peer_id in &tied_bidders {
                if let Some(nonce) = self.verified_nonces.get(*peer_id) {
                    for i in 0..32 {
                        combined_xor[i] ^= nonce[i];
                    }
                }
            }

            // Step 2: Score each tied peer by hashing (r_j || S_tie)
            let mut best_peer = tied_bidders[0];
            let mut highest_score = 0u64;

            for peer_id in &tied_bidders {
                let peer_nonce = self.verified_nonces
                    .get(*peer_id)
                    .copied()
                    .unwrap_or([0u8; 32]);

                let mut hasher = Sha256::new();
                hasher.update(peer_nonce);        // r_j
                hasher.update(combined_xor);      // S_tie
                let digest = hasher.finalize();

                let score = u64::from_le_bytes(digest[..8].try_into().unwrap());

                if score > highest_score {
                    highest_score = score;
                    best_peer = peer_id;
                }
            }

            self.winner_id = Some(best_peer.clone());

            self.clearing_price = valid_bidders
                .iter()
                .find(|&&(id, _)| id != best_peer)
                .map(|&(_, bid)| *bid)
                .unwrap_or(self.reserve_price);
        }
    }
}