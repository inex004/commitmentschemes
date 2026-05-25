use std::error::Error;
use std::time::Duration;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use futures::StreamExt;
use std::sync::{Arc, Mutex}; 

use libp2p::{
    identity, identify, gossipsub,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, SwarmBuilder,
};
use warp::Filter;
use tokio::sync::mpsc;
use bytes::Bytes;

#[derive(NetworkBehaviour)]
struct BridgeBehaviour {
    gossipsub: gossipsub::Behaviour,
    identify: identify::Behaviour, 
}

// 🔥 HELPER FUNCTION: Now safely caches ALL 5 stages (including Commits!)
fn process_auction_message(json: &serde_json::Value, active_auctions: &Arc<Mutex<Vec<serde_json::Value>>>) {
    let mut list = active_auctions.lock().unwrap();
    
    if let Some(announce) = json.get("AnnounceAuction") {
        let new_id = announce["auction_id"].as_str().unwrap_or("").to_string();
        list.retain(|a| a["auction_id"].as_str().unwrap_or("") != new_id);
        
        let mut new_auction = announce.clone();
        if let Some(obj) = new_auction.as_object_mut() {
            obj.insert("bids".to_string(), serde_json::json!([]));
            // 🔥 NEW: Initialize a dictionary to hold the ZK Commitments
            obj.insert("commitments".to_string(), serde_json::json!({}));
            obj.insert("validator_id".to_string(), serde_json::Value::Null);
            obj.insert("verdict".to_string(), serde_json::Value::Null);
        }
        list.push(new_auction);
        println!("📡 [BRIDGE CACHE]: Cached Auction: {}", new_id);
    } 
    // 🔥 NEW: Catch the ZK Commitments!
    else if let Some(commit) = json.get("Commit") {
        let target_id = commit["auction_id"].as_str().unwrap_or("");
        for auction in list.iter_mut() {
            if auction["auction_id"].as_str().unwrap_or("") == target_id {
                if let Some(obj) = auction.as_object_mut() {
                    if let Some(commitments) = obj.get_mut("commitments").and_then(|c| c.as_object_mut()) {
                        let bidder = commit["bidder_id"].as_str().unwrap_or("");
                        let commitment_hex = commit["commitment"].as_str().unwrap_or("");
                        commitments.insert(bidder.to_string(), serde_json::json!(commitment_hex));
                    }
                }
                println!("🔒 [BRIDGE CACHE]: Cached ZK Commitment for auction {}!", target_id);
            }
        }
    }
    else if let Some(reveal) = json.get("Reveal") {
        let target_id = reveal["auction_id"].as_str().unwrap_or("");
        for auction in list.iter_mut() {
            if auction["auction_id"].as_str().unwrap_or("") == target_id {
                if let Some(obj) = auction.as_object_mut() {
                    if let Some(bids) = obj.get_mut("bids").and_then(|b| b.as_array_mut()) {
                        let bidder = reveal["bidder_id"].as_str().unwrap_or("");
                        bids.retain(|b| b["bidder_id"].as_str().unwrap_or("") != bidder);
                        bids.push(reveal.clone());
                    }
                }
                println!("💸 [BRIDGE CACHE]: Attached Reveal bid to auction {}!", target_id);
            }
        }
    }
    else if let Some(intent) = json.get("IntentToValidate") {
        let target_id = intent["auction_id"].as_str().unwrap_or("");
        for auction in list.iter_mut() {
            if auction["auction_id"].as_str().unwrap_or("") == target_id {
                if let Some(obj) = auction.as_object_mut() {
                    if obj.get("validator_id").unwrap_or(&serde_json::Value::Null).is_null() {
                        obj.insert("validator_id".to_string(), intent["validator_id"].clone());
                        println!("🛡️ [BRIDGE CACHE]: Added Validator to Auction: {}", target_id);
                    }
                }
            }
        }
    }
    else if let Some(verdict) = json.get("Verdict") {
        let target_id = verdict["auction_id"].as_str().unwrap_or("");
        for auction in list.iter_mut() {
            if auction["auction_id"].as_str().unwrap_or("") == target_id {
                if let Some(obj) = auction.as_object_mut() {
                    obj.insert("verdict".to_string(), verdict.clone());
                    println!("⚖️ [BRIDGE CACHE]: Attached Verdict to Auction: {}", target_id);
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🌐 Starting Dynamic Mobile API Gateway Bridge...");

    let local_key = identity::Keypair::generate_ed25519();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(100);

    let active_auctions = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let auctions_clone = active_auctions.clone();

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec!["content-type"]);

    let tx_clone = tx.clone();
    let broadcast_route = warp::post()
        .and(warp::path("broadcast"))
        .and(warp::body::bytes())
        .map(move |bytes: Bytes| {
            let _ = tx_clone.try_send(bytes.to_vec());
            warp::reply::with_status("Broadcasted to Mesh", warp::http::StatusCode::OK)
        });

    let get_auctions_route = warp::get()
        .and(warp::path("auctions"))
        .map(move || {
            let list = auctions_clone.lock().unwrap();
            warp::reply::json(&*list)
        });

    let api_route = broadcast_route.or(get_auctions_route).with(cors);

    tokio::spawn(async move {
        println!("🚀 HTTP API gateway listening on port 8080...");
        warp::serve(api_route).run(([0, 0, 0, 0], 8080)).await;
    });

    let mut swarm = SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_quic()
        .with_behaviour(|key| {
            let message_id_fn = |message: &gossipsub::Message| {
                let mut s = DefaultHasher::new();
                message.data.hash(&mut s);
                gossipsub::MessageId::from(s.finish().to_string())
            };
            let gossipsub_config = gossipsub::ConfigBuilder::default()
                .heartbeat_interval(Duration::from_secs(1))
                .validation_mode(gossipsub::ValidationMode::Strict)
                .message_id_fn(message_id_fn)
                .build()
                .unwrap();
            
            let mut gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()),
                gossipsub_config
            ).unwrap();
            
            let topic = gossipsub::IdentTopic::new("energy-auction");
            gossipsub.subscribe(&topic).unwrap();
            
            let identify = identify::Behaviour::new(
                identify::Config::new("/energy-auction/1.0.0".into(), key.public())
            );

            BridgeBehaviour { gossipsub, identify }
        })?
        .build();

    let relay_addr: Multiaddr = "/ip4/127.0.0.1/udp/10000/quic-v1/p2p/12D3KooWFTkBYJMDsxZPD2NENnBGTUwA5BRWEMRuPDUYuV2Mpxgx".parse()?;
    swarm.dial(relay_addr)?;
    println!("🔌 Bridge linked internally to local Relay on port 10000");

    let topic = gossipsub::IdentTopic::new("energy-auction");

    loop {
        tokio::select! {
            Some(msg_bytes) = rx.recv() => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&msg_bytes) {
                    process_auction_message(&json, &active_auctions);
                }
                match swarm.behaviour_mut().gossipsub.publish(topic.clone(), msg_bytes) {
                    Ok(_) => println!("🚀 Packet ACTUALLY injected into the Gossipsub Mesh!"),
                    Err(e) => println!("❌ FATAL BRIDGE PUBLISH ERROR: {:?}", e),
                }
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("🤝 Bridge connection verified with peer: {}", peer_id);
                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                },
                SwarmEvent::Behaviour(BridgeBehaviourEvent::Identify(_)) => {},
                SwarmEvent::Behaviour(BridgeBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })) => {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&message.data) {
                        process_auction_message(&json, &active_auctions);
                    }
                },
                _ => {}
            }
        }
    }
}