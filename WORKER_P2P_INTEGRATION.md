# Worker P2P Integration Status

## ✅ Hoàn thành

### 1. BatchMaker - Gửi batch qua P2P
- ✅ Thêm `peer_mapping: HashMap<PublicKey, PeerId>` để map worker name → PeerId
- ✅ Thêm `tx_req_res: Sender<ReqResCommand>` để gửi P2P requests
- ✅ Thêm `request_counter: u64` để tạo unique request IDs
- ✅ Sửa `seal()` để gửi batch qua P2P thay vì TCP `ReliableSender`
- ✅ BatchMaker giờ gửi `ReqResCommand::SendRequest` với batch data

### 2. QuorumWaiter - Nhận ACK từ P2P
- ✅ Thay đổi `QuorumWaiterMessage`:
  - ❌ Xóa: `handlers: Vec<(PublicKey, CancelHandler)>` (TCP-based)
  - ✅ Thêm: `request_ids: Vec<u64>` - Track requests đã gửi
  - ✅ Thêm: `worker_names: Vec<PublicKey>` - Tên workers để tính stake
- ✅ Thêm `rx_req_res_event: Receiver<ReqResEvent>` - Nhận P2P ACK events
- ✅ Viết lại logic `run()`:
  - Track pending batches với `HashMap<batch_id, (batch, expected_ids, worker_names, received_acks)>`
  - Nhận `ReqResEvent::ResponseReceived` (ACK)
  - Tính tổng stake từ workers đã ACK
  - Forward batch khi đạt quorum threshold

### 3. Worker struct
- ✅ Thêm fields:
  - `tx_req_res: Option<Sender<ReqResCommand>>` - Gửi P2P commands
  - `tx_req_res_event_to_quorum: Option<Sender<ReqResEvent>>` - Forward ACKs to QuorumWaiter
  - `peer_mapping: HashMap<PublicKey, PeerId>` - Map PublicKey → PeerId
- ✅ Cập nhật `Worker::spawn()` signature để nhận P2P channels

### 4. Dependencies
- ✅ Thêm vào `worker/Cargo.toml`:
  - `p2p = { path = "../p2p" }`
  - `libp2p = "0.53"`

## ⚠️ Cần hoàn thiện

### 1. P2P Event Routing trong Worker
**Vấn đề:** P2P event loop phát ra 2 loại events, nhưng Worker cần route chúng đến 2 nơi khác nhau:
- `ReqResEvent::ResponseReceived` (ACK) → `QuorumWaiter` 
- `ReqResEvent::RequestReceived` (batch từ worker khác) → `Processor`

**Giải pháp cần implement:**

```rust
// Trong Worker::handle_clients_transactions:
let (tx_req_res_event_for_quorum, rx_req_res_event_for_quorum) = channel(CHANNEL_CAPACITY);

// Trong Worker::handle_workers_messages:
let (tx_req_res_event_for_processor, rx_req_res_event_for_processor) = channel(CHANNEL_CAPACITY);

// Tạo một task P2P event router
tokio::spawn(async move {
    while let Some(event) = rx_req_res_event.recv().await {
        match event {
            // ACK events → QuorumWaiter
            ReqResEvent::ResponseReceived { .. } => {
                tx_req_res_event_for_quorum.send(event).await;
            }
            // Request events → Processor
            ReqResEvent::RequestReceived { request_id, from_peer, data } => {
                // Deserialize batch và forward vào Processor
                tx_processor.send(data).await;
            }
            _ => {}
        }
    }
});
```

### 2. Peer Discovery & Mapping
**Vấn đề:** BatchMaker cần `peer_mapping: HashMap<PublicKey, PeerId>` để biết gửi batch đến PeerId nào.

**Giải pháp:**
- Trong `main.rs`, track mDNS discovered peers
- Map Committee's PublicKey → PeerId (có thể dùng derived keypair hoặc config file)
- Pass peer_mapping vào Worker::spawn()

**Code cần thêm vào main.rs:**
```rust
// Track peers discovered qua mDNS/Kad
let mut peer_mapping: HashMap<PublicKey, PeerId> = HashMap::new();

// Trong P2P event loop, khi discover peer mới:
match event {
    MyBehaviourEvent::Mdns(MdnsEvent::Discovered(peers)) => {
        for (peer_id, _addr) in peers {
            // TODO: Lấy PublicKey từ peer_id (cần protocol handshake)
            // peer_mapping.insert(public_key, peer_id);
        }
    }
    ...
}
```

### 3. Worker Test Updates
**File:** `worker/src/tests/worker_tests.rs`

Cần cập nhật:
```rust
Worker::spawn(
    name, 
    id, 
    committee.clone(), 
    parameters, 
    store,
    None,  // tx_req_res
    None,  // tx_req_res_event_to_quorum
    HashMap::new(),  // peer_mapping
);
```

### 4. Primary Integration
Nếu Primary cũng dùng Worker, cần cập nhật `Primary::spawn()` để truyền P2P channels.

## 📋 Kiến trúc hiện tại

```
┌─────────────────────────────────────────────────────────────┐
│                      P2P Event Loop                          │
│  - Nhận ReqResCommand từ BatchMaker                          │
│  - Gửi ReqResEvent (ACK + Requests) ra channel              │
└──────────────────┬──────────────────────────────────────────┘
                   │
                   │ rx_req_res_event
                   ▼
┌─────────────────────────────────────────────────────────────┐
│                   P2P Event Router                           │
│                   (CẦN IMPLEMENT)                            │
│  - ResponseReceived → tx_req_res_event_for_quorum          │
│  - RequestReceived → tx_processor (deserialize batch)       │
└──────────┬────────────────────────────┬─────────────────────┘
           │                            │
           │ ACKs                       │ Batches
           ▼                            ▼
┌──────────────────────┐    ┌──────────────────────────┐
│   QuorumWaiter       │    │      Processor           │
│  - Track ACKs        │    │  - Store batch           │
│  - Calculate stake   │    │  - Hash & report digest  │
│  - Forward on quorum │    │  - Send to Primary       │
└──────────────────────┘    └──────────────────────────┘
```

## 🔧 Next Steps

1. **Implement P2P Event Router** trong `Worker::spawn()`
   - Tách ACK events → QuorumWaiter
   - Tách Request events → Processor

2. **Implement Peer Discovery**
   - Track discovered peers trong main.rs
   - Build PublicKey → PeerId mapping
   - Pass vào Worker::spawn()

3. **Update Tests**
   - Fix worker_tests.rs với signature mới
   - Thêm P2P integration tests

4. **Remove TCP Fallback** (optional)
   - Xóa `ReliableSender` khỏi BatchMaker
   - Xóa TCP-based `Receiver::spawn` cho worker-to-worker
   - Chỉ giữ lại P2P-based communication

## 📝 Notes

- ✅ P2P Request-Response protocol hoàn toàn functional (đã test trong demo)
- ✅ Automatic ACK sending đã implement trong `req_res_handler.rs`
- ✅ Generic protocol support bất kỳ data type (Vec<u8>)
- ⚠️ Cần implement routing logic để tách loại events
- ⚠️ Cần peer discovery mechanism để populate peer_mapping
