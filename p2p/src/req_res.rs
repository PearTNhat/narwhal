// p2p/src/req_res.rs
// Generic Request-Response implementation với ACK tracking

use libp2p::{
    request_response::{
        self, Codec, ProtocolSupport,
    },
    PeerId, StreamProtocol,
};
use serde::{Deserialize, Serialize};
use std::io;
use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};

/// Generic Request với data bất kỳ
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericRequest {
    pub request_id: u64,
    pub data: Vec<u8>,
}

/// Generic Response (ACK)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericResponse {
    pub request_id: u64,
    pub success: bool,
    pub message: String,
}

/// Codec cho Generic Request-Response
#[derive(Debug, Clone, Default)]
pub struct GenericCodec;

#[async_trait]
impl Codec for GenericCodec {
    type Protocol = StreamProtocol;
    type Request = GenericRequest;
    type Response = GenericResponse;

    async fn read_request<T>(
        &mut self,
        _protocol: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        use futures::AsyncReadExt;
        
        // Đọc length (4 bytes)
        let mut len_buf = [0u8; 4];
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        // Đọc data
        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;

        // Deserialize
        bincode::deserialize(&buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Deserialize error: {}", e))
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
        use futures::AsyncReadExt;
        
        let mut len_buf = [0u8; 4];
        io.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut buf = vec![0u8; len];
        io.read_exact(&mut buf).await?;

        bincode::deserialize(&buf).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Deserialize error: {}", e))
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
            io::Error::new(io::ErrorKind::InvalidData, format!("Serialize error: {}", e))
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
            io::Error::new(io::ErrorKind::InvalidData, format!("Serialize error: {}", e))
        })?;

        let len = (data.len() as u32).to_be_bytes();
        io.write_all(&len).await?;
        io.write_all(&data).await?;
        io.flush().await?;
        Ok(())
    }
}

/// Tạo Request-Response Behaviour
pub fn create_request_response_behaviour() -> request_response::Behaviour<GenericCodec> {
    let protocols = std::iter::once((
        StreamProtocol::new("/narwhal/req-res/1.0.0"),
        ProtocolSupport::Full,
    ));
    
    request_response::Behaviour::new(protocols, request_response::Config::default())
}

/// Event từ P2P gửi cho application logic
#[derive(Debug, Clone)]
pub enum ReqResEvent {
    /// Nhận được response (ACK) từ peer
    ResponseReceived {
        request_id: u64,
        from_peer: PeerId,
        success: bool,
        message: String,
    },
    /// Request bị fail (timeout, connection error)
    RequestFailed {
        request_id: u64,
        to_peer: PeerId,
        error: String,
    },
    /// Nhận request từ peer khác (cần gửi response)
    RequestReceived {
        request_id: u64,
        from_peer: PeerId,
        data: Vec<u8>,
    },
}

/// Command từ application gửi cho P2P
#[derive(Debug, Clone)]
pub enum ReqResCommand {
    /// Gửi request đến peer cụ thể
    SendRequest {
        request_id: u64,
        target_peer: PeerId,
        data: Vec<u8>,
    },
}
