// src/p2p/types.rs
use libp2p::{
    gossipsub::{self, Behaviour as Gossipsub},
    identify::{Behaviour as Identify, Event as IdentifyEvent},
    kad::{store::MemoryStore, Behaviour as Kademlia, Event as KademliaEvent},
    mdns::{tokio::Behaviour as Mdns, Event as MdnsEvent},
    request_response,
    swarm::NetworkBehaviour,
};
use serde::{Deserialize, Serialize};
use crate::req_res::GenericCodec;

// Cấu trúc tin nhắn chung để broadcast
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericMessage {
    pub sender_id: String,
    pub content: String,
    pub timestamp: u64,
}

impl GenericMessage {
    pub fn new(sender_id: String, content: String) -> Self {
        Self {
            sender_id,
            content,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

// Giữ trạng thái cho việc cố gắng kết nối lại với một peer
#[derive(Debug, Clone, Default)]
pub struct ReconnectState {
    pub attempts: u32,
    pub is_pending: bool,
}

// Tập hợp tất cả các hành vi mạng của libp2p vào một struct duy nhất.
#[derive(NetworkBehaviour)]
#[behaviour(out_event = "MyBehaviourEvent")]
pub struct MyBehaviour {
    pub kad: Kademlia<MemoryStore>,
    pub mdns: Mdns,
    pub gossipsub: Gossipsub,
    pub identify: Identify,
    pub req_res: request_response::Behaviour<GenericCodec>,
}

// Enum để gom tất cả các sự kiện từ các hành vi khác nhau.
#[derive(Debug)]
pub enum MyBehaviourEvent {
    Kad(KademliaEvent),
    Mdns(MdnsEvent),
    Gossipsub(gossipsub::Event),
    Identify(IdentifyEvent),
    ReqRes(request_response::Event<crate::req_res::GenericRequest, crate::req_res::GenericResponse>),
}

impl From<KademliaEvent> for MyBehaviourEvent {
    fn from(event: KademliaEvent) -> Self {
        MyBehaviourEvent::Kad(event)
    }
}

impl From<MdnsEvent> for MyBehaviourEvent {
    fn from(event: MdnsEvent) -> Self {
        MyBehaviourEvent::Mdns(event)
    }
}

impl From<gossipsub::Event> for MyBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        MyBehaviourEvent::Gossipsub(event)
    }
}

impl From<IdentifyEvent> for MyBehaviourEvent {
    fn from(event: IdentifyEvent) -> Self {
        MyBehaviourEvent::Identify(event)
    }
}

impl From<request_response::Event<crate::req_res::GenericRequest, crate::req_res::GenericResponse>> for MyBehaviourEvent {
    fn from(event: request_response::Event<crate::req_res::GenericRequest, crate::req_res::GenericResponse>) -> Self {
        MyBehaviourEvent::ReqRes(event)
    }
}


// Enum cấp cao nhất để đóng gói tất cả các loại message có thể gửi qua mạng.
// Giúp việc deserialize ở phía nhận trở nên an toàn và rõ ràng.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum P2pMessage {
    Primary(PrimaryMessage1),
    Worker(WorkerMessage1),
}
// Enum cho các message liên quan đến consensus giữa các Primary.
// BẠN SẼ THÊM CÁC LOẠI MESSAGE THỰC TẾ VÀO ĐÂY (VÍ DỤ: HEADER, VOTE, CERTIFICATE)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum PrimaryMessage1 {
    // Ví dụ:
    // Header(Vec<u8>), // Chứa header đã được serialize
    // Vote(Vec<u8>),   // Chứa vote đã được serialize
    Hello(String), // Giữ lại để demo
    // Request-Response pattern
    Request { request_id: u64, data: String },
    Response { request_id: u64, data: String },
}

// Enum cho các message liên quan đến giao tiếp giữa các Worker.
// BẠN SẼ THÊM CÁC LOẠI MESSAGE THỰC TẾ VÀO ĐÂY (VÍ DỤ: BATCH REQUEST)
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WorkerMessage1 {
    // Ví dụ:
    // RequestBatch(Vec<u8>),
    Hello(String), // Giữ lại để demo
    // Request-Response pattern
    Request { request_id: u64, data: String },
    Response { request_id: u64, data: String },
}

// -- Topics --

/// Trả về topic cho việc giao tiếp giữa các Primary node.
pub fn get_primary_topic() -> gossipsub::IdentTopic {
    gossipsub::IdentTopic::new("narwhal-primary-consensus")
}

/// Trả về topic cho việc giao tiếp giữa các Worker node.
pub fn get_worker_topic() -> gossipsub::IdentTopic {
    gossipsub::IdentTopic::new("narwhal-worker-sync")
}

