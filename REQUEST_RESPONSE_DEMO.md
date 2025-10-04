# Demo Request-Response với ACK Tracking

## 🎯 Tổng quan

Demo này show case cách sử dụng libp2p Request-Response protocol để:
1. Gửi request đến peer cụ thể
2. Peer tự động gửi ACK response
3. Track và log latency của request

## 🏗️ Kiến trúc

```
┌──────────────────────────────────────────────────────────────┐
│                         Application                           │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  // Gửi request                                              │
│  tx_req_res_command.send(ReqResCommand::SendRequest {       │
│      request_id: 1,                                          │
│      target_peer: peer_id,                                   │
│      data: vec![...],                                        │
│  }).await;                                                   │
│                                                               │
│  // Nhận ACK                                                 │
│  while let Some(event) = rx_req_res_event.recv().await {    │
│      match event {                                           │
│          ReqResEvent::ResponseReceived { request_id, .. } => {│
│              log::info!("✅ ACK received for #{}", request_id);│
│          }                                                    │
│      }                                                        │
│  }                                                            │
│                                                               │
└───────────────────────┬──────────────────────────────────────┘
                        │
                        │ Channel (mpsc)
                        │
┌───────────────────────▼──────────────────────────────────────┐
│                   P2P Event Loop                              │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  tokio::select! {                                            │
│      // Nhận command từ app                                  │
│      Some(cmd) = rx_req_res_command.recv() => {             │
│          swarm.behaviour_mut()                               │
│              .req_res                                        │
│              .send_request(&peer, request);                  │
│      }                                                        │
│                                                               │
│      // Xử lý swarm event                                    │
│      event = swarm.select_next_some() => {                   │
│          match event {                                       │
│              ReqRes(Message::Response { response, .. }) => { │
│                  // Forward ACK event to app                 │
│                  tx_req_res_event.send(ResponseReceived {    │
│                      request_id: response.request_id,        │
│                      ...                                     │
│                  }).await;                                   │
│              }                                               │
│          }                                                    │
│      }                                                        │
│  }                                                            │
│                                                               │
└───────────────────────┬──────────────────────────────────────┘
                        │
                        │ libp2p Request-Response Protocol
                        │
                ┌───────▼────────┐
                │   Other Peer   │
                │                │
                │  Auto-send ACK │
                └────────────────┘
```

## 📝 API Design

### 1. Command (App → P2P)

```rust
pub enum ReqResCommand {
    SendRequest {
        request_id: u64,
        target_peer: PeerId,
        data: Vec<u8>,
    },
}
```

**Ví dụ sử dụng:**
```rust
// Gửi request
let data = b"Hello, peer!".to_vec();
tx_req_res_command.send(ReqResCommand::SendRequest {
    request_id: 123,
    target_peer: peer_id,
    data,
}).await?;
```

### 2. Event (P2P → App)

```rust
pub enum ReqResEvent {
    /// Nhận được ACK từ peer
    ResponseReceived {
        request_id: u64,
        from_peer: PeerId,
        success: bool,
        message: String,
    },
    
    /// Request failed (timeout, connection error)
    RequestFailed {
        request_id: u64,
        to_peer: PeerId,
        error: String,
    },
    
    /// Nhận request từ peer khác
    RequestReceived {
        request_id: u64,
        from_peer: PeerId,
        data: Vec<u8>,
    },
}
```

**Ví dụ xử lý:**
```rust
while let Some(event) = rx_req_res_event.recv().await {
    match event {
        ReqResEvent::ResponseReceived { request_id, from_peer, success, message } => {
            let latency = pending_requests.remove(&request_id).unwrap().elapsed();
            log::info!("✅ ACK for Request #{} from {} in {:?}", 
                request_id, from_peer, latency);
        }
        ReqResEvent::RequestFailed { request_id, to_peer, error } => {
            log::warn!("❌ Request #{} to {} failed: {}", 
                request_id, to_peer, error);
        }
        ReqResEvent::RequestReceived { request_id, from_peer, data } => {
            log::info!("📨 Received Request #{} from {}", request_id, from_peer);
            // ACK đã tự động gửi
        }
    }
}
```

## 🔄 Luồng hoạt động

### Gửi Request và nhận ACK:

```
Time  │ Node A (Sender)              │ Node B (Receiver)
──────┼──────────────────────────────┼───────────────────────────────
  0ms │ Send Request #1              │
      │ ├─ Store: pending[1] = now   │
      │ └─ swarm.send_request()      │
      │                              │
      │        ──Request──>          │
      │                              │
 10ms │                              │ Receive Request #1
      │                              │ ├─ Log: "📨 Received Request"
      │                              │ ├─ Auto-send ACK
      │                              │ └─ swarm.send_response()
      │                              │
      │        <──Response──         │
      │                              │
 20ms │ Receive Response #1          │
      │ ├─ latency = 20ms            │
      │ ├─ Remove: pending[1]        │
      │ └─ Log: "✅ ACK in 20ms"     │
```

## 🚀 Cách chạy Demo

### Bước 1: Build
```bash
cd /home/abc/nhat/narwhal
cargo build --release
```

### Bước 2: Chạy 2 nodes

**Terminal 1 - Node 0:**
```bash
RUST_LOG=info ./target/release/node run \
  --keys committee.json \
  --committee committee.json \
  --store db_primary_0 \
  primary
```

**Terminal 2 - Node 1:**
```bash
RUST_LOG=info ./target/release/node run \
  --keys committee.json \
  --committee committee.json \
  --store db_primary_1 \
  primary
```

### Hoặc dùng script:
```bash
chmod +x demo_req_res_ack.sh
./demo_req_res_ack.sh
```

## 📊 Log Output mẫu

```
[NODE-0] 📌 P2P Peer ID for Primary: 12D3KooWABC...
[NODE-0] ✅ P2P listening on port 9000
[NODE-0] P ___Subscribed to PRIMARY topic: narwhal-primary-consensus

[NODE-1] 📌 P2P Peer ID for Primary: 12D3KooWXYZ...
[NODE-1] ✅ P2P listening on port 9001
[NODE-1] P ___Subscribed to PRIMARY topic: narwhal-primary-consensus

[NODE-0] 🔍 mDNS discovered peer: 12D3KooWXYZ... at /ip4/127.0.0.1/...
[NODE-1] 🔍 mDNS discovered peer: 12D3KooWABC... at /ip4/127.0.0.1/...

[NODE-0] 📤 [REQ-RES] Sending Request #1 to peer 12D3KooWXYZ...
[NODE-1] 📨 [REQ-RES] Received Request #1 from peer 12D3KooWABC...
[NODE-1] ✅ [REQ-RES] Sent ACK for Request #1
[NODE-0] ✅ [REQ-RES] ACK received for Request #1
[NODE-0]     ├─ From: 12D3KooWXYZ...
[NODE-0]     ├─ Success: true
[NODE-0]     ├─ Message: 'ACK from 12D3KooWXYZ...'
[NODE-0]     └─ Latency: 15ms
```

## 🔧 Tích hợp vào Worker

Để áp dụng vào Worker BatchMaker:

```rust
// In BatchMaker::seal()
async fn seal(&mut self) {
    let batch_id = hash_batch(&self.current_batch);
    let batch_data = bincode::serialize(&self.current_batch).unwrap();
    
    // Gửi requests đến tất cả workers
    for (worker_name, worker_peer_id) in &self.worker_peers {
        let request_id = self.next_request_id();
        
        // Track pending request
        self.pending_batches.insert(request_id, PendingBatch {
            batch_id: batch_id.clone(),
            stake: self.committee.stake(worker_name),
            sent_at: Instant::now(),
        });
        
        // Gửi request
        self.tx_req_res_command.send(ReqResCommand::SendRequest {
            request_id,
            target_peer: *worker_peer_id,
            data: batch_data.clone(),
        }).await;
    }
}

// Trong event handler
while let Some(event) = rx_req_res_event.recv().await {
    match event {
        ReqResEvent::ResponseReceived { request_id, from_peer, .. } => {
            if let Some(pending) = self.pending_batches.remove(&request_id) {
                // Tính quorum
                self.batch_acks.entry(pending.batch_id)
                    .or_insert_with(QuorumTracker::new)
                    .add_ack(pending.stake);
                
                // Check if reached quorum
                if self.batch_acks[&pending.batch_id].reached_quorum() {
                    // Forward to processor
                    self.tx_processor.send(pending.batch_id).await;
                }
            }
        }
    }
}
```

## ✅ Ưu điểm

1. **Type-safe API**: Command và Event được định nghĩa rõ ràng
2. **Reusable**: Có thể dùng cho bất kỳ data nào (Vec<u8>)
3. **Async-friendly**: Sử dụng channels, không block
4. **Latency tracking**: Dễ dàng measure performance
5. **Automatic ACK**: Handler tự động gửi response
6. **Error handling**: RequestFailed event cho timeout/errors

## 📚 Files liên quan

- `p2p/src/req_res.rs` - Protocol definitions
- `p2p/src/req_res_handler.rs` - Event handler
- `p2p/src/event_loop.rs` - Event loop integration
- `node/src/main.rs` - Demo usage
- `demo_req_res_ack.sh` - Run script
