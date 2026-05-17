use std::error::Error;
use std::time::Duration;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use futures::StreamExt;
use std::sync::{Arc, Mutex}; // 🔥 NEW: Required for shared memory

use libp2p::{
    identity, gossipsub,
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, SwarmBuilder,
};
use warp::Filter;
use tokio::sync::mpsc;
use bytes::Bytes;

#[derive(NetworkBehaviour)]
struct BridgeBehaviour {
    gossipsub: gossipsub::Behaviour,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🌐 Starting Dynamic Mobile API Gateway Bridge...");

    let local_key = identity::Keypair::generate_ed25519();
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(100);

    // 🔥 NEW: Shared memory cache to hold live auctions for the mobile phones
    let active_auctions = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let auctions_clone = active_auctions.clone();

    let cors = warp::cors()
        .allow_any_origin()
        .allow_methods(vec!["GET", "POST", "OPTIONS"]) // 🔥 Allowed GET
        .allow_headers(vec!["content-type"]);

    let tx_clone = tx.clone();
    let broadcast_route = warp::post()
        .and(warp::path("broadcast"))
        .and(warp::body::bytes())
        .map(move |bytes: Bytes| {
            let _ = tx_clone.try_send(bytes.to_vec());
            warp::reply::with_status("Broadcasted to Mesh", warp::http::StatusCode::OK)
        });

    // 🔥 NEW: Route for mobile phones to fetch the live auction list
    let get_auctions_route = warp::get()
        .and(warp::path("auctions"))
        .map(move || {
            let list = auctions_clone.lock().unwrap();
            warp::reply::json(&*list)
        });

    // Combine both routes
    let api_route = broadcast_route.or(get_auctions_route).with(cors);

    tokio::spawn(async move {
        println!("🚀 HTTP API gateway listening on port 8080...");
        warp::serve(api_route).run(([0, 0, 0, 0], 8080)).await;
    });

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
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
            
            BridgeBehaviour { gossipsub }
        })?
        .build();

    let relay_addr: Multiaddr = "/ip4/127.0.0.1/udp/10000/quic-v1/p2p/12D3KooWFTkBYJMDsxZPD2NENnBGTUwA5BRWEMRuPDUYuV2Mpxgx".parse()?;
    swarm.dial(relay_addr)?;
    println!("🔌 Bridge linked internally to local Relay on port 10000");

    let topic = gossipsub::IdentTopic::new("energy-auction");

    loop {
        tokio::select! {
            Some(msg_bytes) = rx.recv() => {
                println!("📥 Forwarding Web HTTP Bid -> Injected into P2P Gossip Mesh!");
                let _ = swarm.behaviour_mut().gossipsub.publish(topic.clone(), msg_bytes);
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    println!("🤝 Bridge connection verified with peer: {}", peer_id);
                    // 🔥 NEW: Tell the Mobile Bridge to join the Mesh too!
                    swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                },
                // 🔥 NEW: Listen to the mesh and save new auctions to the memory cache!
                SwarmEvent::Behaviour(BridgeBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. })) => {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&message.data) {
                        if let Some(announce) = json.get("AnnounceAuction") {
                            let mut list = active_auctions.lock().unwrap();
                            let new_id = announce["auction_id"].as_str().unwrap_or("").to_string();
                            
                            // Remove old copies if they exist, then add the new one
                            list.retain(|a| a["auction_id"].as_str().unwrap_or("") != new_id);
                            list.push(announce.clone());
                            
                            println!("📡 [BRIDGE CACHE]: Saved new auction for mobile web view: {}", new_id);
                        }
                    }
                },
                _ => {}
            }
        }
    }
}