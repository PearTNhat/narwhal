// p2p/src/req_res_handler.rs
// Handler cho Request-Response events

use crate::req_res::{GenericRequest, GenericResponse, ReqResEvent};
use crate::types::MyBehaviour;
use libp2p::{
    request_response::{Event, Message},
    PeerId, Swarm,
};
use log::{info, warn};
use tokio::sync::mpsc;

/// Xử lý Request-Response events
pub fn handle_req_res_event(
    event: Event<GenericRequest, GenericResponse>,
    swarm: &mut Swarm<MyBehaviour>,
    tx_req_res_event: &mpsc::Sender<ReqResEvent>,
    our_peer_id: &PeerId,
) {
    match event {
        // Nhận message từ peer
        Event::Message { peer, message } => {
            match message {
                // Nhận request từ peer khác
                Message::Request {
                    request_id: _,
                    request,
                    channel,
                } => {
                    info!(
                        "📨 [REQ-RES] Received Request #{} from peer {}",
                        request.request_id, peer
                    );

                    // Tự động gửi ACK response
                    let response = GenericResponse {
                        request_id: request.request_id,
                        success: true,
                        message: format!("ACK from {}", our_peer_id),
                    };

                    if let Err(e) = swarm
                        .behaviour_mut()
                        .req_res
                        .send_response(channel, response.clone())
                    {
                        warn!(
                            "❌ Failed to send response for request #{}: {:?}",
                            request.request_id, e
                        );
                    } else {
                        info!(
                            "✅ [REQ-RES] Sent ACK for Request #{}",
                            request.request_id
                        );
                    }

                    // Forward event đến application logic
                    let event = ReqResEvent::RequestReceived {
                        request_id: request.request_id,
                        from_peer: peer,
                        data: request.data,
                    };

                    let tx = tx_req_res_event.clone();
                    tokio::spawn(async move {
                        if tx.send(event).await.is_err() {
                            warn!("Failed to forward RequestReceived event to application");
                        }
                    });
                }

                // Nhận response (ACK) từ peer
                Message::Response {
                    request_id: _,
                    response,
                } => {
                    info!(
                        "✅ [REQ-RES] Received Response for Request #{} from peer {}: success={}, message='{}'",
                        response.request_id, peer, response.success, response.message
                    );

                    // Forward event đến application logic
                    let event = ReqResEvent::ResponseReceived {
                        request_id: response.request_id,
                        from_peer: peer,
                        success: response.success,
                        message: response.message,
                    };

                    let tx = tx_req_res_event.clone();
                    tokio::spawn(async move {
                        if tx.send(event).await.is_err() {
                            warn!("Failed to forward ResponseReceived event to application");
                        }
                    });
                }
            }
        }

        // Request failed (timeout, connection error, etc.)
        Event::OutboundFailure {
            peer,
            request_id,
            error,
        } => {
            warn!(
                "❌ [REQ-RES] Outbound request {:?} to {} failed: {:?}",
                request_id, peer, error
            );

            // Forward failure event
            let event = ReqResEvent::RequestFailed {
                request_id: 0, // We don't track request_id in outbound failure
                to_peer: peer,
                error: format!("{:?}", error),
            };

            let tx = tx_req_res_event.clone();
            tokio::spawn(async move {
                if tx.send(event).await.is_err() {
                    warn!("Failed to forward RequestFailed event to application");
                }
            });
        }

        // Inbound failure
        Event::InboundFailure {
            peer,
            request_id,
            error,
        } => {
            warn!(
                "❌ [REQ-RES] Inbound request {:?} from {} failed: {:?}",
                request_id, peer, error
            );
        }

        // Response sent successfully
        Event::ResponseSent { peer, request_id } => {
            info!(
                "📤 [REQ-RES] Response sent successfully for request {:?} to {}",
                request_id, peer
            );
        }
    }
}
