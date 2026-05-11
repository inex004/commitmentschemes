use libp2p::{
    gossipsub, identify, identity, noise, yamux, relay, autonat, dcutr, ping, 
    kad, kad::store::MemoryStore, upnp, // 🔥 NEW: Imported upnp
    swarm::NetworkBehaviour,
    PeerId, SwarmBuilder, Transport,
};
use libp2p::core::upgrade;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum NetworkMessage {
    AnnounceAuction { auction_id: String, seller_id: String, energy_amount: u64, reserve_price: u64 },
    IntentToValidate { auction_id: String, validator_id: String },
    Verdict { auction_id: String, validator_id: String, winner_id: Option<String>, clearing_price: u64, slash_list: Vec<String> },
    Commit { auction_id: String, bidder_id: String, binding_hash: String },
    Reveal { auction_id: String, bidder_id: String, bid: u64, blind_hex: String },
    Heartbeat { auction_id: String, seller_id: String },
    DeliveryComplete { auction_id: String, seller_id: String },
    
    // 🔥 NEW: The Signaling Message for our custom hole puncher
    NatSignal { peer_id: String, public_ip: String },
}

#[derive(NetworkBehaviour)]
pub struct AuctionNetworkBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub identify: identify::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub autonat: autonat::Behaviour,
    pub ping: ping::Behaviour,
    pub kad: kad::Behaviour<MemoryStore>, 
    pub upnp: upnp::tokio::Behaviour, // 🔥 NEW: Added UPnP Network Behaviour
}

pub fn setup_swarm(
    id_keys: identity::Keypair,
    local_peer_id: PeerId,
) -> Result<libp2p::Swarm<AuctionNetworkBehaviour>, Box<dyn std::error::Error>> {

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
        .expect("Valid gossipsub config");

    let mut gossipsub = gossipsub::Behaviour::new(
        gossipsub::MessageAuthenticity::Signed(id_keys.clone()),
        gossipsub_config,
    ).expect("Valid gossipsub behaviour");

    let topic = gossipsub::IdentTopic::new("energy-auction");
    gossipsub.subscribe(&topic)?;

    let identify = identify::Behaviour::new(identify::Config::new("/energy-auction/1.0.0".into(), id_keys.public()));
    
    let (relay_transport, relay_client) = relay::client::new(local_peer_id);

    let store = MemoryStore::new(local_peer_id);
    let mut kademlia = kad::Behaviour::new(local_peer_id, store);
    kademlia.set_mode(Some(kad::Mode::Client));

    let swarm = SwarmBuilder::with_existing_identity(id_keys.clone())
        .with_tokio()
        .with_quic() 
        .with_other_transport(|key| {
            relay_transport
                .upgrade(upgrade::Version::V1)
                .authenticate(noise::Config::new(key).unwrap())
                .multiplex(yamux::Config::default())
                .boxed()
        })?
        .with_behaviour(|_| AuctionNetworkBehaviour {
            gossipsub,
            identify,
            relay_client,
            dcutr: dcutr::Behaviour::new(local_peer_id),
            autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
            ping: ping::Behaviour::new(ping::Config::new()),
            kad: kademlia, 
            upnp: upnp::tokio::Behaviour::default(), // 🔥 NEW: Initialize UPnP mapping
        })?
        .with_swarm_config(|c: libp2p::swarm::Config| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    Ok(swarm)
}