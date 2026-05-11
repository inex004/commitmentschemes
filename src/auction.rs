use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub struct Auction {
    pub auction_id: String,
    pub seller_id: String,
    pub energy_amount: u64,
    pub reserve_price: u64,
    pub commit_deadline: u64,
    pub reveal_deadline: u64, 
    pub received_commitments: HashMap<String, String>,
    pub verified_bids: HashMap<String, u64>,
    pub verified_blinds: HashMap<String, [u8; 32]>,
    pub resolved: bool,
    pub failed: bool,
    pub winner_id: Option<String>,
    pub clearing_price: u64,
    pub slash_list: Vec<String>, 
    
    // 🔥 NEW: Validator and Consensus Tracking
    pub validator_id: Option<String>,
    pub verdict_received: bool,

    // Physical flow tracking
    pub is_delivering: bool,
    pub energy_delivered: u64,
    pub last_heartbeat: u64,
    pub slashed: bool,
}

pub struct MarketplaceState {
    pub my_credits: u64,
    pub my_energy: u64,
    pub my_locked_credits: u64,
    pub my_locked_energy: u64,
    pub active_auctions: HashMap<String, Auction>,
    pub current_joined_auction: Option<String>,
    pub my_secret_bid: Option<u64>,
    pub my_secret_blind: Option<curve25519_dalek::scalar::Scalar>,
    pub unplugged_meters: std::collections::HashSet<String>,
}

impl MarketplaceState {
    pub fn new() -> Self {
        Self {
            my_credits: 1000,
            my_energy: 500,
            my_locked_credits: 0,
            my_locked_energy: 0,
            active_auctions: HashMap::new(),
            current_joined_auction: None,
            my_secret_bid: None,
            my_secret_blind: None,
            unplugged_meters: std::collections::HashSet::new(),
        }
    }
}

impl Auction {
    pub fn new(auction_id: String, seller_id: String, energy_amount: u64, reserve_price: u64) -> Self {
        let parts: Vec<&str> = auction_id.split('_').collect();
        let start_time = if parts.len() == 2 {
            parts[1].parse::<u64>().unwrap_or_else(|_| SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs())
        } else {
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        };

        Self {
            auction_id,
            seller_id,
            energy_amount,
            reserve_price,
            commit_deadline: start_time + 240, 
            reveal_deadline: start_time + 300, 
            received_commitments: HashMap::new(),
            verified_bids: HashMap::new(),
            verified_blinds: HashMap::new(),
            resolved: false,
            failed: false,
            winner_id: None,
            clearing_price: 0,
            slash_list: Vec::new(),
            validator_id: None,
            verdict_received: false,
            is_delivering: false,
            energy_delivered: 0,
            last_heartbeat: start_time,
            slashed: false,
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

        let mut valid_bidders: Vec<(&String, &u64)> = self.verified_bids.iter()
            .filter(|(_, &bid)| bid >= self.reserve_price)
            .collect();

        if valid_bidders.is_empty() {
            self.failed = true;
            return;
        }

        valid_bidders.sort_by(|a, b| b.1.cmp(a.1));
        let highest_bid = *valid_bidders[0].1;

        let tied_bidders: Vec<&String> = valid_bidders.iter()
            .filter(|&&(_, bid)| *bid == highest_bid)
            .map(|&(id, _)| id)
            .collect();

        if tied_bidders.len() == 1 {
            self.winner_id = Some(tied_bidders[0].clone());
            self.clearing_price = if valid_bidders.len() > 1 { *valid_bidders[1].1 } else { self.reserve_price };
            self.is_delivering = true;
            self.last_heartbeat = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        } else {
            println!("⚖️ TIE DETECTED! Executing Instant XOR Tie-Breaker among honest revealers...");
            let mut combined_xor = [0u8; 32];
            for peer_id in &tied_bidders {
                if let Some(blind) = self.verified_blinds.get(*peer_id) {
                    for i in 0..32 { combined_xor[i] ^= blind[i]; }
                }
            }
            let xor_seed = hex::encode(combined_xor);

            let mut best_peer = tied_bidders[0];
            let mut highest_score = 0;

            for peer in tied_bidders {
                let mut hasher = DefaultHasher::new();
                format!("{}{}", xor_seed, peer).hash(&mut hasher);
                let score = hasher.finish();
                if score > highest_score {
                    highest_score = score;
                    best_peer = peer;
                }
            }

            self.winner_id = Some(best_peer.clone());
            self.clearing_price = highest_bid; 
            self.is_delivering = true;
            self.last_heartbeat = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        }
    }
}