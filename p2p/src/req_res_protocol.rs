// p2p/src/req_res_protocol.rs
// Libp2p Request-Response protocol implementation for Narwhal

use libp2p::{
    request_response::{
        self, Behaviour, Codec, Event, Message, ProtocolSupport, ResponseChannel,
    },
    PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::io;
use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};

/// Protocol name for batch distribution
pub const BATCH_PROTOCOL: &str = "/narwhal/batch/1.0.0";

/// Request: Worker gửi batch đến các worker khác
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRequest {
    pub batch_data: Vec<u8>,
    pub batch_id: Vec<u8>, // Digest
    pub worker_id: u32,
}

/// Response: Worker khác trả lời ACK
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchResponse {
    pub batch_id: Vec<u8>,
    pub ack: bool,
    pub responder: Vec<u8>, // PublicKey serialized
}

/// Codec để serialize/deserialize messages
#[derive(Debug, Clone)]
pub struct BatchCodec;

#[async_trait]
impl Codec for BatchCodec {
    type Protocol = StreamProtocol;
    type Request = BatchRequest;
    type Response = BatchResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // Đọc length prefix (4 bytes)
        let mut len_buf = [0u8; 4];
        use futures::AsyncReadExt;
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Đọc data
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;

        // Deserialize
        bincode::deserialize(&buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, e)
        })
    }

    async fn read_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut len_buf = [0u8; 4];
        use futures::AsyncReadExt;
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;

        bincode::deserialize(&buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, e)
        })
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        let data = bincode::serialize(&req).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;

        let len = (data.len() as u32).to_be_bytes();
        io.write_all(&len).await?;
        io.write_all(&data).await?;
        io.flush().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        use futures::AsyncWriteExt;
        let data = bincode::serialize(&res).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?;

        let len = (data.len() as u32).to_be_bytes();
        io.write_all(&len).await?;
        io.write_all(&data).await?;
        io.flush().await?;
        Ok(())
    }
}

/// Tạo Request-Response Behaviour
pub fn create_batch_behaviour() -> Behaviour<BatchCodec> {
    let protocols = std::iter::once((
        StreamProtocol::new(BATCH_PROTOCOL),
        ProtocolSupport::Full,
    ));
    
    let cfg = request_response::Config::default();
    
    Behaviour::new(protocols, cfg)
}

/// Message để track pending requests (giống CancelHandler)
#[derive(Debug, Clone)]
pub struct PendingBatchRequest {
    pub batch_id: Vec<u8>,
    pub request_id: request_response::OutboundRequestId,
    pub target_peer: PeerId,
}

/// Message gửi từ Worker logic đến P2P event loop
#[derive(Debug, Clone)]
pub enum WorkerP2pCommand {
    /// Broadcast batch đến danh sách peers và đợi ACK
    BroadcastBatch {
        batch_data: Vec<u8>,
        batch_id: Vec<u8>,
        worker_id: u32,
        target_peers: Vec<PeerId>,
    },
}

/// Event trả về từ P2P event loop cho Worker logic
#[derive(Debug, Clone)]
pub enum WorkerP2pEvent {
    /// Nhận được ACK từ một peer
    BatchAck {
        batch_id: Vec<u8>,
        from_peer: PeerId,
        responder_pubkey: Vec<u8>,
    },
    /// Request failed (timeout, connection error, etc.)
    BatchRequestFailed {
        batch_id: Vec<u8>,
        to_peer: PeerId,
        error: String,
    },
    /// Nhận batch request từ peer khác (cần gửi ACK)
    BatchReceived {
        batch_data: Vec<u8>,
        batch_id: Vec<u8>,
        worker_id: u32,
        from_peer: PeerId,
        response_channel: ResponseChannel<BatchResponse>,
    },
}
