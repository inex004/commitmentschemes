use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

/// Attempts to punch a hole through a Symmetric NAT by "spraying" UDP packets.
/// Returns the exact `IP:Port` string of the peer if successful.
pub fn execute_port_spray(target_ip: &str, start_port: u16, end_port: u16) -> Option<String> {
    // 🔥 NEW: Parse the target IP to determine if it is IPv4 or IPv6
    let target_ip_parsed: IpAddr = target_ip.parse().expect("Invalid IP address format");

    // 🔥 NEW: Dynamically bind the correct socket type based on the target!
    let bind_addr = if target_ip_parsed.is_ipv6() {
        "[::]:0" // IPv6 "Any" address
    } else {
        "0.0.0.0:0" // IPv4 "Any" address
    };

    let socket = UdpSocket::bind(bind_addr).expect("Failed to bind raw UDP socket");
    socket.set_nonblocking(true).expect("Failed to set non-blocking mode");

    let local_port = socket.local_addr().unwrap().port();
    println!("🎯 Local raw UDP socket bound to port: {}", local_port);
    println!("🚀 Initiating UDP Port Spray against {} (Ports {} to {})", target_ip, start_port, end_port);

    let timeout = Duration::from_secs(5);
    let start_time = Instant::now();

    // ==========================================
    // PHASE 1: THE SPRAY
    // ==========================================
    for port in start_port..=end_port {
        // 🔥 NEW: Use SocketAddr instead of format!("{}:{}") to handle IPv6 brackets automatically!
        let target_addr = SocketAddr::new(target_ip_parsed, port);
        
        let _ = socket.send_to(b"KNOCK", target_addr);
        thread::sleep(Duration::from_millis(2)); 
    }

    println!("💦 Spray complete. Listening closely for incoming punches...");

    // ==========================================
    // PHASE 2: THE LISTEN
    // ==========================================
    let mut buffer = [0u8; 1024];
    
    while start_time.elapsed() < timeout {
        if let Ok((size, src_addr)) = socket.recv_from(&mut buffer) {
            let received_msg = String::from_utf8_lossy(&buffer[..size]);
            
            if received_msg.contains("KNOCK") {
                println!("✅ HOLE PUNCH SUCCESSFUL!");
                println!("🔗 Peer broke through your NAT from: {}", src_addr);
                return Some(src_addr.to_string());
            }
        }
        thread::sleep(Duration::from_millis(10));
    }

    println!("❌ Hole punch failed. The Symmetric NAT did not yield.");
    None
}