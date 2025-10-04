# Tóm tắt: Tích hợp P2P vào Worker (Worker-to-Worker Communication)

## 🎯 Mục tiêu đã đạt được

Thay thế TCP-based communication bằng libp2p Request-Response protocol cho Worker-to-Worker batch broadcasting.

## 📦 Files đã thay đổi

### 1. `/worker/Cargo.toml`
```toml
+ p2p = { path = "../p2p" }
+ libp2p = "0.53"
```

### 2. `/worker/src/batch_maker.rs`
**Thay đổi chính:**
- Thêm imports: `libp2p::PeerId`, `p2p::ReqResCommand`
- Thêm fields vào `BatchMaker`:
  - `peer_mapping: HashMap<PublicKey, PeerId>` - Map worker name → P2P peer ID
  - `tx_req_res: Sender<ReqResCommand>` - Channel gửi P2P commands
  - `request_counter: u64` - Tạo unique request IDs
- Sửa `spawn()` signature: thêm parameters cho P2P
- **Viết lại `seal()` method:**
  ```rust
  // CŨ: Gửi qua TCP với ReliableSender
  let handlers = self.network.broadcast(addresses, bytes).await;
  
  // MỚI: Gửi qua P2P Request-Response
  for name in &names {
      if let Some(peer_id) = self.peer_mapping.get(name) {
          self.request_counter += 1;
          let cmd = ReqResCommand::SendRequest {
              request_id: self.request_counter,
              target_peer: *peer_id,
              data: serialized.clone(),
          };
          self.tx_req_res.send(cmd).await;
      }
  }
  ```

### 3. `/worker/src/quorum_waiter.rs`
**Thay đổi chính:**
- Thêm imports: `p2p::ReqResEvent`, `std::collections::{HashMap, HashSet}`
- **Thay đổi `QuorumWaiterMessage`:**
  ```rust
  // CŨ:
  pub handlers: Vec<(PublicKey, CancelHandler)>  // TCP ACK handlers
  
  // MỚI:
  pub request_ids: Vec<u64>        // Track P2P request IDs
  pub worker_names: Vec<PublicKey> // Tính stake từ workers
  ```
- Thêm field vào `QuorumWaiter`:
  - `rx_req_res_event: Receiver<ReqResEvent>` - Nhận P2P ACK events
- **Viết lại `run()` method:**
  ```rust
  // CŨ: Chờ CancelHandler futures hoàn thành (TCP)
  let mut wait_for_quorum: FuturesUnordered<_> = handlers
      .into_iter()
      .map(|(name, handler)| Self::waiter(handler, stake))
      .collect();
  
  // MỚI: Nhận P2P events từ channel
  loop {
      tokio::select! {
          Some(batch_msg) = self.rx_message.recv() => {
              // Track pending batch
          }
          Some(event) = self.rx_req_res_event.recv() => {
              match event {
                  ReqResEvent::ResponseReceived { request_id, .. } => {
                      // Tính stake, check quorum
                  }
              }
          }
      }
  }
  ```

### 4. `/worker/src/worker.rs`
**Thay đổi chính:**
- Thêm imports: `libp2p::PeerId`, `p2p::{ReqResCommand, ReqResEvent}`, `std::collections::HashMap`
- Thêm fields vào `Worker`:
  - `tx_req_res: Option<Sender<ReqResCommand>>`
  - `tx_req_res_event_to_quorum: Option<Sender<ReqResEvent>>`
  - `peer_mapping: HashMap<PublicKey, PeerId>`
- Sửa `spawn()` signature: thêm P2P channels parameters
- Sửa `handle_clients_transactions()`:
  - Tạo channel cho QuorumWaiter ACK events
  - Pass P2P channels vào `BatchMaker::spawn()`
  - Pass ACK channel vào `QuorumWaiter::spawn()`

### 5. `/node/src/main.rs`
**Thay đổi chính:**
- Cập nhật Worker::spawn call:
  ```rust
  Worker::spawn(
      keypair.name, 
      id, 
      committee, 
      parameters, 
      store,
      Some(tx_req_res_command.clone()),  // P2P command channel
      None,                               // ACK event channel
      peer_mapping,                       // PublicKey → PeerId mapping
  );
  ```

## 🔄 Flow mới (P2P-based)

```
Client Transaction
       ↓
   BatchMaker
       ├─→ Gom transactions thành batch
       ├─→ Serialize batch
       └─→ Gửi P2P Request đến mỗi worker:
           ReqResCommand::SendRequest {
               request_id: unique_id,
               target_peer: worker_peer_id,
               data: serialized_batch
           }
       ↓
   P2P Event Loop (p2p/event_loop.rs)
       ├─→ Nhận ReqResCommand
       ├─→ Gửi request qua libp2p
       └─→ Nhận response từ remote workers
       ↓
   P2P Event Handler (p2p/req_res_handler.rs)
       ├─→ Remote worker tự động gửi ACK
       └─→ Phát ReqResEvent::ResponseReceived
       ↓
   QuorumWaiter
       ├─→ Nhận ACK events
       ├─→ Track received_acks per batch
       ├─→ Tính total_stake từ ACK workers
       └─→ Khi đạt quorum threshold:
           Forward batch → Processor
       ↓
   Processor
       ├─→ Hash batch
       ├─→ Store vào RocksDB
       └─→ Gửi digest lên Primary
```

## ⚠️ Limitations hiện tại

### 1. Peer Discovery chưa hoàn thiện
- `peer_mapping: HashMap<PublicKey, PeerId>` hiện tại là empty
- **Cần implement:** Map committee's PublicKey → discovered PeerId

### 2. P2P Event Routing chưa hoàn chỉnh
Worker nhận 2 loại P2P events:
- **ACK events** (ResponseReceived) → cần route đến QuorumWaiter
- **Request events** (RequestReceived) → cần route đến Processor

**Giải pháp:** Tạo router task trong Worker:
```rust
tokio::spawn(async move {
    while let Some(event) = rx_req_res_event.recv().await {
        match event {
            ReqResEvent::ResponseReceived { .. } => {
                tx_to_quorum.send(event).await;
            }
            ReqResEvent::RequestReceived { data, .. } => {
                tx_processor.send(data).await;
            }
            _ => {}
        }
    }
});
```

### 3. TCP Receiver vẫn còn
- `Receiver::spawn(address, WorkerReceiverHandler)` vẫn đang chạy
- **Lý do giữ lại:** Backward compatibility
- **Có thể xóa** khi confirm P2P hoạt động 100%

## ✅ Ưu điểm của P2P approach

1. **Tự động ACK:** libp2p request-response protocol tự động gửi response
2. **NAT Traversal:** Hole punching, relay qua TURN servers
3. **Peer Discovery:** mDNS, Kademlia DHT
4. **Multiplexing:** Nhiều protocols chạy trên cùng connection
5. **Retry & Timeout:** Built-in vào libp2p
6. **Connection pooling:** Tái sử dụng connections

## 🚀 Next Steps

1. **Implement Peer Discovery:**
   ```rust
   // Trong P2P event loop, track discovered peers
   match event {
       MyBehaviourEvent::Mdns(MdnsEvent::Discovered(peers)) => {
           for (peer_id, _addr) in peers {
               // Map peer_id → PublicKey (cần handshake protocol)
               peer_mapping.insert(public_key, peer_id);
           }
       }
   }
   ```

2. **Implement P2P Event Router** trong Worker

3. **Remove TCP fallback** (optional)

4. **Testing:**
   - Run multi-node cluster
   - Measure latency vs TCP
   - Test Byzantine scenarios (worker crashes, network partition)

## 📚 Related Documentation

- [REQUEST_RESPONSE_DEMO.md](REQUEST_RESPONSE_DEMO.md) - API usage guide
- [WORKER_P2P_INTEGRATION.md](WORKER_P2P_INTEGRATION.md) - Detailed status
- [REQ_RES_SUMMARY.md](REQ_RES_SUMMARY.md) - Protocol summary
