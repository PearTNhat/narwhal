// src/p2p/func.rs
use super::types::{MyBehaviour};
use libp2p::{
    gossipsub::{
        self, Behaviour as Gossipsub, ConfigBuilder, MessageAuthenticity,
        ValidationMode,
    }, identify::Behaviour as Identify, identity::{self}, kad::{store::MemoryStore, Behaviour as Kademlia}, mdns::tokio::Behaviour as Mdns, Multiaddr, PeerId, Swarm, SwarmBuilder
};
use crypto::PublicKey; 
use std::{
    error::Error,
    hash::{Hash, Hasher},
    str::FromStr,
    time::Duration,
};
use std::collections::hash_map::DefaultHasher;

/// Tạo libp2p keypair từ seed đơn giản (cho demo)
pub fn create_keypair_from_seed(seed: u8) -> identity::Keypair {
    let mut key_bytes = [0u8; 32];
    key_bytes[0] = seed;
    // Điền thêm một số byte để tránh key quá yếu
    for i in 1..32 {
        key_bytes[i] = seed.wrapping_add(i as u8);
    }
    let secret_key = identity::secp256k1::SecretKey::try_from_bytes(key_bytes)
        .expect("Failed to create secp256k1 secret key");
    let secp_keypair = identity::secp256k1::Keypair::from(secret_key);
    identity::Keypair::from(secp_keypair)
}

/// Tạo libp2p keypair một cách có thể xác định (deterministic) từ một public key gốc và một ID thành phần (component).
/// Điều này đảm bảo Primary và các Worker của cùng một node sẽ có PeerID khác nhau.
/// - component_id: Có thể là WorkerId, hoặc một giá trị đặc biệt cho Primary (ví dụ: u32::MAX).
pub fn create_derived_keypair(base_public_key: &PublicKey, component_id: u32) -> identity::Keypair {
    // 1. Tạo một nguồn dữ liệu gốc (seed material) duy nhất
    // bằng cách kết hợp public key gốc và component_id.
    let mut hasher = DefaultHasher::new();
    base_public_key.0.hash(&mut hasher); // Dùng toàn bộ 32 byte của public key
    component_id.hash(&mut hasher);     // Thêm ID của component vào
    let hash_result = hasher.finish();

    // 2. Sử dụng kết quả hash để tạo ra một seed 32-byte cho secret key.
    // Chúng ta sẽ lặp lại 8 byte của hash 4 lần để lấp đầy 32 byte.
    // Đây là một cách đơn giản để tạo seed có tính quyết định và duy nhất.
    let hash_bytes = hash_result.to_le_bytes(); // 8 bytes
    let mut key_bytes = [0u8; 32];
    for i in 0..4 {
        key_bytes[i*8..(i+1)*8].copy_from_slice(&hash_bytes);
    }
    // 3. Tạo secret key và keypair từ seed đã tạo.
    let secret_key = identity::secp256k1::SecretKey::try_from_bytes(key_bytes)
        .expect("Failed to create secp256k1 secret key from derived seed");
    let secp_keypair = identity::secp256k1::Keypair::from(secret_key);
    identity::Keypair::from(secp_keypair)
}


// Tạo một Swarm libp2p với các hành vi được cấu hình.
pub async fn create_p2p_swarm(
    local_key: identity::Keypair,
    local_peer_id: PeerId,
) -> Result<Swarm<MyBehaviour>, Box<dyn Error + Send + Sync>> {
    let message_id_fn = |message: &gossipsub::Message| {
        let mut s = DefaultHasher::new();
        message.data.hash(&mut s);
        gossipsub::MessageId::from(s.finish().to_string())
    };
    let gossipsub_config = ConfigBuilder::default()
        .heartbeat_interval(Duration::from_secs(10))
        .validation_mode(ValidationMode::Strict)
        .message_id_fn(message_id_fn)
        .build()?;
    let gossipsub_behaviour = Gossipsub::new(
        MessageAuthenticity::Signed(local_key.clone()),
        gossipsub_config,
    )?;
    let store = MemoryStore::new(local_peer_id);
    let kad_behaviour = Kademlia::new(local_peer_id, store);
    let mdns_behaviour = Mdns::new(libp2p::mdns::Config::default(), local_peer_id)?;
    let identify_behaviour = Identify::new(libp2p::identify::Config::new(
        "/ipfs/id/1.0.0".to_string(),
        local_key.public(),
    ));

    let behaviour = MyBehaviour {
        kad: kad_behaviour,
        mdns: mdns_behaviour,
        gossipsub: gossipsub_behaviour,
        identify: identify_behaviour,
    };

    let swarm = SwarmBuilder::with_existing_identity(local_key)
        .with_tokio()
        .with_quic()
        .with_behaviour(|_| behaviour)?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    Ok(swarm)
}

/// Kết nối đến các nút bootstrap.
pub async fn bootstrap_network(
    swarm: &mut Swarm<MyBehaviour>,
    nodes: &[String],
) -> Result<(), Box<dyn Error + Send + Sync>> {
    for addr_str in nodes {
        let remote_addr: Multiaddr = addr_str.parse()?;
        let remote_peer_id_str = remote_addr
            .iter()
            .find_map(|p| match p {
                libp2p::multiaddr::Protocol::P2p(peer_id) => Some(peer_id),
                _ => None,
            })
            .ok_or("Bootstrap address must include PeerId")?;
        let remote_peer_id = PeerId::from_str(&remote_peer_id_str.to_string())?;
        swarm
            .behaviour_mut()
            .kad
            .add_address(&remote_peer_id, remote_addr);
        let _ = swarm.behaviour_mut().kad.bootstrap();
    }
    Ok(())
}
