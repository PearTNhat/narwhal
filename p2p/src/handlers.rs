// src/p2p/handlers.rs

use super::types::{MyBehaviour, MyBehaviourEvent};
use crate::{
    P2pMessage, PrimaryMessage1, WorkerMessage1,
    types::{get_primary_topic, get_worker_topic},
};
use libp2p::{
    PeerId, gossipsub,
    identify::Event as IdentifyEvent,
    kad::{Event as KademliaEvent, QueryResult},
    mdns::Event as MdnsEvent,
    swarm::{Swarm, SwarmEvent},
};
use tokio::sync::mpsc;
use log::{info, warn};

pub fn handle_swarm_event(
    event: SwarmEvent<MyBehaviourEvent>,
    swarm: &mut Swarm<MyBehaviour>,
    tx_to_primary: &mpsc::Sender<PrimaryMessage1>,
    tx_to_worker: &mpsc::Sender<WorkerMessage1>,
) {
    match event {
        SwarmEvent::NewListenAddr { address, .. } => {
            let full_addr = address.clone().with_p2p(*swarm.local_peer_id()).unwrap();
            info!("🌐 P2P Node listening on: {}", full_addr);
        }
        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
            info!("✅ P2P Connection established with peer: {}", peer_id);
        }
        SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
            warn!(
                "❌ P2P Connection lost with peer: {}. Cause: {:?}",
                peer_id, cause
            );
            swarm
                .behaviour_mut()
                .gossipsub
                .remove_explicit_peer(&peer_id);
            swarm.behaviour_mut().kad.remove_peer(&peer_id);
        }
        SwarmEvent::Behaviour(MyBehaviourEvent::Mdns(event)) => handle_mdns_event(event, swarm),
        SwarmEvent::Behaviour(MyBehaviourEvent::Kad(event)) => handle_kademlia_event(event),
        SwarmEvent::Behaviour(MyBehaviourEvent::Gossipsub(event)) => {
            handle_gossipsub_event(event, tx_to_primary, tx_to_worker);
        }
        SwarmEvent::Behaviour(MyBehaviourEvent::Identify(event)) => {
            if let IdentifyEvent::Received { peer_id, info } = event {
                info!(
                    "✨ Identified peer {}: agent={}",
                    peer_id, info.agent_version
                );
            }
        }
        _ => {}
    }
}

/// Xử lý các sự kiện Gossipsub.
fn handle_gossipsub_event(
    event: gossipsub::Event,
    tx_to_primary: &mpsc::Sender<PrimaryMessage1>,
    tx_to_worker: &mpsc::Sender<WorkerMessage1>,
) {
    if let gossipsub::Event::Message { message, .. } = event {
        let primary_topic_hash = get_primary_topic().hash();
        let worker_topic_hash = get_worker_topic().hash();
        // So sánh trực tiếp các TopicHash
        if message.topic == primary_topic_hash {
            match bincode::deserialize::<P2pMessage>(&message.data) {
                Ok(P2pMessage::Primary(primary_msg)) => {
                    let tx = tx_to_primary.clone();
                    tokio::spawn(async move {
                        if tx.send(primary_msg).await.is_err() {
                            warn!("Failed to forward P2P message to Primary logic");
                        }
                    });
                }
                _ => warn!(
                    "❌ Received a message on the primary topic, but it was not a PrimaryMessage type."
                ),
            }
        } else if message.topic == worker_topic_hash {
            match bincode::deserialize::<P2pMessage>(&message.data) {
                Ok(P2pMessage::Worker(worker_msg)) => {
                    let tx = tx_to_worker.clone();
                    tokio::spawn(async move {
                        if tx.send(worker_msg).await.is_err() {
                            warn!("Failed to forward P2P message to Worker logic");
                        }
                    });
                }
                _ => warn!(
                    "❌ Received a message on the worker topic, but it was not a WorkerMessage type."
                ),
            }
        } else {
            // Ghi log cho các topic không xác định để dễ debug
            warn!(
                "Received message on an unknown topic hash: {:?}",
                message.topic
            );
        }
    }
}

/// Xử lý các sự kiện Kademlia.
fn handle_kademlia_event(event: KademliaEvent) {
    if let KademliaEvent::OutboundQueryProgressed {
        result: QueryResult::Bootstrap(Ok(result)),
        ..
    } = event
    {
        info!(
            "Bootstrap finished successfully. Peers found: {}",
            result.num_remaining
        );
    } else if let KademliaEvent::RoutingUpdated { peer, .. } = event {
        info!("Routing table updated for peer: {}", peer);
    }
}

/// Xử lý các sự kiện mDNS.
fn handle_mdns_event(event: MdnsEvent, swarm: &mut Swarm<MyBehaviour>) {
    match event {
        MdnsEvent::Discovered(list) => {
            for (peer_id, multiaddr) in list {
                info!("🔍 mDNS discovered peer: {} at {}", peer_id, multiaddr);
                swarm.behaviour_mut().kad.add_address(&peer_id, multiaddr);
                swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
            }
        }
        MdnsEvent::Expired(list) => {
            for (peer_id, _multiaddr) in list {
                info!("⏰ mDNS peer expired: {}", peer_id);
                swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);
            }
        }
    }
}
