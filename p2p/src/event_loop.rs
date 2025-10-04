// src/p2p/event_loop.rs
use crate::{
    handlers::handle_swarm_event,
    types::{MyBehaviour, P2pMessage, PrimaryMessage1, WorkerMessage1},
    req_res::{ReqResCommand, ReqResEvent, GenericRequest},
};
use futures::StreamExt;
use libp2p::{gossipsub, Swarm};
use tokio::sync::mpsc;
use log::{info, warn};

/// Chạy vòng lặp sự kiện P2P
/// - rx_from_core: nhận message từ Core/Primary để publish lên gossipsub
/// - tx_to_primary: gửi message nhận được từ gossipsub đến Primary
/// - rx_req_res_command: nhận request-response commands
/// - tx_req_res_event: gửi request-response events
pub async fn run_p2p_event_loop(
    mut swarm: Swarm<MyBehaviour>,
    mut rx_from_core: mpsc::Receiver<(gossipsub::IdentTopic, P2pMessage)>,
    tx_to_primary: mpsc::Sender<PrimaryMessage1>,
    tx_to_worker: mpsc::Sender<WorkerMessage1>,
    mut rx_req_res_command: mpsc::Receiver<ReqResCommand>,
    tx_req_res_event: mpsc::Sender<ReqResEvent>,
) {
    let our_peer_id = *swarm.local_peer_id();
    loop {
        tokio::select! {
            // Xử lý sự kiện từ Swarm (kết nối, nhận message, etc.)
            event = swarm.select_next_some() => {
                handle_swarm_event(event, &mut swarm, &tx_to_primary, &tx_to_worker, &tx_req_res_event, &our_peer_id);
            },
            // Nhận message từ logic chính để publish ra mạng
            Some((topic, message)) = rx_from_core.recv() => {
                match bincode::serialize(&message) {
                   Ok(serialized_message) => {
                        if let Err(e) = swarm.behaviour_mut().gossipsub.publish(topic.clone(), serialized_message) {
                            warn!("❌ Failed to publish message to topic {}: {}", topic, e);
                        } else {
                            info!("✅ Published message to topic {}", topic);
                        }
                    },
                    Err(e) => warn!("❌ Failed to serialize P2P message: {}", e),
                }
            }
            
            // Nhận Request-Response command từ application
            Some(cmd) = rx_req_res_command.recv() => {
                match cmd {
                    ReqResCommand::SendRequest { request_id, target_peer, data } => {
                        info!("📤 [REQ-RES] Sending Request #{} to peer {}", request_id, target_peer);
                        
                        let request = GenericRequest {
                            request_id,
                            data,
                        };
                        
                        let _req_id = swarm.behaviour_mut().req_res.send_request(&target_peer, request);
                    }
                }
            }
        
            else => {
                warn!("⚠️ P2P event loop ended");
                break;
            }
        }
    }
}
