use std::error::Error;
use std::fs;
use std::time::Duration; 
use std::collections::hash_map::DefaultHasher; // 🔥 NEW: Needed for Gossipsub
use std::hash::{Hash, Hasher};                 // 🔥 NEW: Needed for Gossipsub

use libp2p::{
    identity, identify, relay, ping, 
    kad, kad::store::MemoryStore, 
    gossipsub, // 🔥 NEW: Import Gossipsub
    swarm::{NetworkBehaviour, SwarmEvent},
    Multiaddr, SwarmBuilder,
};
use futures::StreamExt;

const RELAY_KEY_FILE: &str = "relay_node.key";

#[derive(NetworkBehaviour)]
struct RelayBehaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    kad: kad::Behaviour<MemoryStore>, 
    gossipsub: gossipsub::Behaviour, // 🔥 NEW: The Relay is now a PubSub Broker!
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("🚀 Starting Dedicated Cloud Relay Node (QUIC/UDP)...");

    let local_key = if let Ok(bytes) = fs::read(RELAY_KEY_FILE) {
        println!("🔑 Loaded existing relay keypair from disk.");
        identity::Keypair::from_protobuf_encoding(&bytes)?
    } else {
        println!("🔑 No keypair found. Generating new one...");
        let new_key = identity::Keypair::generate_ed25519();
        fs::write(RELAY_KEY_FILE, new_key.to_protobuf_encoding()?)?;
        new_key
    };

    let local_peer_id = local_key.public().to_peer_id();
    println!("🌍 Relay Peer ID: {}", local_peer_id);

    let mut swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_quic() 
        .with_behaviour(|key| {
            let relay_config = relay::Config {
                max_reservations: 8192,           
                max_reservations_per_peer: 100,   
                max_circuits: 1024,               
                max_circuits_per_peer: 100,       
                reservation_duration: Duration::from_secs(86400),
                max_circuit_duration: Duration::from_secs(86400),     
                max_circuit_bytes: u64::MAX,                          
                ..Default::default()
            };

            let store = MemoryStore::new(local_peer_id);
            let mut kademlia = kad::Behaviour::new(local_peer_id, store);
            kademlia.set_mode(Some(kad::Mode::Server)); 

            // 🔥 NEW: Setup Gossipsub for the Relay
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
                .expect("Valid config");
            
            let mut gossipsub = gossipsub::Behaviour::new(
                gossipsub::MessageAuthenticity::Signed(key.clone()), 
                gossipsub_config
            ).expect("Correct configuration");

            // 🔥 NEW: The Relay MUST subscribe to the auction topic to repeat the messages!
            let topic = gossipsub::IdentTopic::new("energy-auction");
            gossipsub.subscribe(&topic).unwrap();

            RelayBehaviour {
                relay: relay::Behaviour::new(local_peer_id, relay_config),
                identify: identify::Behaviour::new(identify::Config::new("/energy-auction/1.0.0".into(), key.public())),
                ping: ping::Behaviour::new(ping::Config::new()),
                kad: kademlia, 
                gossipsub, // 🔥 Connect the PubSub Broker to the Swarm
            }
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(86400)))
        .build();

    let listen_addr: Multiaddr = "/ip4/0.0.0.0/udp/10000/quic-v1".parse()?;
    swarm.listen_on(listen_addr)?;

    let listen_addr_ipv6: Multiaddr = "/ip6/::/udp/10000/quic-v1".parse()?;
    swarm.listen_on(listen_addr_ipv6)?;

    let external_addr_ipv4: Multiaddr = "/ip4/34.28.68.14/udp/10000/quic-v1".parse()?;
    swarm.add_external_address(external_addr_ipv4);
    println!("🌐 Hardcoded Public External IPv4 to: 34.28.68.14 (UDP)");

    let external_addr_ipv6: Multiaddr = "/ip6/2600:1900:4001:523:0:0:0:0/udp/10000/quic-v1".parse()?;
    swarm.add_external_address(external_addr_ipv6);
    println!("🌐 Hardcoded Public External IPv6 to: 2600:1900:4001:523:0:0:0:0 (UDP)");

    println!("⏳ Waiting for QUIC connections...");

    loop {
        tokio::select! {
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => println!("🟢 Relay Listening on: {:?}", address),
                SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                    println!("🤝 Peer connected via QUIC: {}", peer_id);
                    swarm.behaviour_mut().kad.add_address(&peer_id, endpoint.get_remote_address().clone());
                },
                SwarmEvent::Behaviour(RelayBehaviourEvent::Relay(relay_event)) => {
                    match relay_event {
                        relay::Event::ReservationReqAccepted { src_peer_id, .. } => {
                            println!("🎟️ [RELAY]: Granted Reservation routing slot to peer {}", &src_peer_id.to_string()[0..8]);
                        },
                        relay::Event::CircuitReqAccepted { src_peer_id, dst_peer_id } => {
                            println!("🔗 [RELAY]: Bridging a p2p-circuit between {} and {}", &src_peer_id.to_string()[0..8], &dst_peer_id.to_string()[0..8]);
                        },
                        relay::Event::ReservationReqDenied { src_peer_id } => {
                            println!("❌ [RELAY]: Denied reservation for {}", &src_peer_id.to_string()[0..8]);
                        },
                        _ => {}
                    }
                },
                // 🔥 NEW: See the Cloud Relay in action when it bridges your mobile and home networks!
                SwarmEvent::Behaviour(RelayBehaviourEvent::Gossipsub(gossipsub::Event::Message { propagation_source, .. })) => {
                    println!("📻 [PUB-SUB TOWER]: Safely routing a Gossipsub message from {} to all connected peers!", &propagation_source.to_string()[0..8]);
                },
                _ => {}
            }
        }
    }
}