use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use libp2p::identity::{Keypair, PublicKey, PeerId};
use serde::{Deserialize, Serialize};

/// The Cryptographic Envelope that wraps every single UDP packet.
#[derive(Serialize, Deserialize, Debug)]
pub struct SignedPacket {
    pub sender_pubkey_bytes: Vec<u8>, // Who I am
    pub signature: Vec<u8>,           // Cryptographic proof
    pub payload: Vec<u8>,             // The actual data (auction bid, etc.)
}

pub struct MagicSocket {
    socket: UdpSocket,
    my_keys: Keypair,
    // 🔥 THE MAGIC ROUTING TABLE: Maps Cryptographic Identity to Physical Location
    routing_table: HashMap<PeerId, SocketAddr>, 
}

impl MagicSocket {
    /// Binds a new raw UDP socket and attaches your Ed25519 identity to it
    pub fn new(bind_port: u16, my_keys: Keypair) -> Self {
        // 🔥 NEW: Bind to `[::]` to enable Dual-Stack. 
        // We add a fallback to `0.0.0.0` just in case the host machine has IPv6 completely disabled.
        let socket = UdpSocket::bind(format!("[::]:{}", bind_port))
            .unwrap_or_else(|_| {
                println!("⚠️ IPv6 not available on this machine. Falling back to IPv4 Magicsock.");
                UdpSocket::bind(format!("0.0.0.0:{}", bind_port)).expect("Failed to bind UDP")
            });
            
        socket.set_nonblocking(true).unwrap();
        
        Self {
            socket,
            my_keys,
            routing_table: HashMap::new(),
        }
    }

    /// Manually add a peer's known IP to the routing table (e.g., from the Relay)
    pub fn add_peer_route(&mut self, peer_id: PeerId, addr: SocketAddr) {
        self.routing_table.insert(peer_id, addr);
    }

    /// Send data to a PeerId. The socket looks up their current IP automatically.
    pub fn send_to_peer(&self, target_peer: &PeerId, payload: Vec<u8>) {
        if let Some(current_ip) = self.routing_table.get(target_peer) {
            // 1. Sign the payload using our Private Key
            let signature = self.my_keys.sign(&payload).expect("Failed to sign payload");
            
            // 2. Package it in the envelope
            let packet = SignedPacket {
                sender_pubkey_bytes: self.my_keys.public().encode_protobuf(), // 🔥 FIXED
                signature,
                payload,
            };

            // 3. Blast it over raw UDP
            let packet_bytes = bincode::serialize(&packet).unwrap();
            let _ = self.socket.send_to(&packet_bytes, current_ip);
        } else {
            println!("⚠️ Cannot send: {} is not in our routing table!", target_peer);
        }
    }

    /// Call this in a loop to process incoming packets and handle IP Roaming
    pub fn poll_incoming(&mut self) -> Option<(PeerId, Vec<u8>)> {
        let mut buffer = [0u8; 65535]; // Max UDP packet size

        if let Ok((size, src_addr)) = self.socket.recv_from(&mut buffer) {
            // 1. Unpack the envelope
            if let Ok(packet) = bincode::deserialize::<SignedPacket>(&buffer[..size]) {
                
                // 2. Extract their Public Key
                if let Ok(pubkey) = PublicKey::try_decode_protobuf(&packet.sender_pubkey_bytes) {
                    let sender_peer_id = pubkey.to_peer_id();

                    // 3. VERIFY THE SIGNATURE (The most important step)
                    if pubkey.verify(&packet.payload, &packet.signature) {
                        
                        // 🔥 CONNECTION ROAMING LOGIC 🔥
                        // Did this verified packet come from a new IP address?
                        let is_new_ip = match self.routing_table.get(&sender_peer_id) {
                            Some(known_addr) => *known_addr != src_addr,
                            None => true, // We've never seen them before
                        };

                        if is_new_ip {
                            println!("🌐 [ROAMING EVENT]: Peer {} moved to a new IP: {}!", 
                                     &sender_peer_id.to_string()[0..8], src_addr);
                            
                            // Instantly update the routing table. 
                            // All future sends to this PeerId will now go to this new IP!
                            self.routing_table.insert(sender_peer_id, src_addr);
                        }

                        // Return the verified data to the main application
                        return Some((sender_peer_id, packet.payload));
                    } else {
                        println!("🚨 [SECURITY ALERT]: Invalid signature from {}!", src_addr);
                    }
                }
            }
        }
        None
    }
}