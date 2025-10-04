pub mod event_loop;
pub mod functions;
pub mod handlers;
pub mod receiver;
pub mod sender;
pub mod state;
pub mod types;
// pub mod req_res_protocol;
pub mod req_res;
pub mod req_res_handler;
// pub mod quorum_waiter_p2p;

// Re-export các thành phần quan trọng để dễ dàng truy cập từ bên ngoài
pub use functions::{create_p2p_swarm, create_keypair_from_seed};
pub use types::{P2pMessage, PrimaryMessage1, WorkerMessage1, MyBehaviour};
pub use req_res::{ReqResCommand, ReqResEvent, GenericRequest, GenericResponse};