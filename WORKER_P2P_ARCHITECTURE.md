# Worker P2P Architecture Diagram

## Batch Broadcasting Flow (Worker → Worker)

```
┌────────────────────────────────────────────────────────────────────────────┐
│                              Worker A (Local)                               │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  Client Transactions                                                        │
│         ↓                                                                   │
│   ┌─────────────┐                                                          │
│   │ BatchMaker  │                                                          │
│   │  - Gom txs  │                                                          │
│   │  - Serialize│                                                          │
│   └──────┬──────┘                                                          │
│          │ batch serialized                                                │
│          ↓                                                                  │
│   ┌──────────────────────────────────────────────────────┐                │
│   │ For each worker in committee:                        │                │
│   │   request_id = counter++                             │                │
│   │   ReqResCommand::SendRequest {                       │                │
│   │     request_id,                                      │                │
│   │     target_peer: peer_mapping[worker_public_key],   │                │
│   │     data: batch.clone()                              │                │
│   │   }                                                  │                │
│   └──────────────────────────┬───────────────────────────┘                │
│                              │ tx_req_res                                  │
│                              ↓                                              │
│   ┌──────────────────────────────────────────────────────┐                │
│   │          P2P Event Loop (event_loop.rs)              │                │
│   │  - Nhận ReqResCommand                                │                │
│   │  - swarm.behaviour_mut().req_res.send_request()     │                │
│   └──────────────────────────┬───────────────────────────┘                │
│                              │                                              │
└──────────────────────────────┼──────────────────────────────────────────────┘
                               │ libp2p Request-Response Protocol
                               │ (over QUIC/TCP)
            ┌──────────────────┼──────────────────┐
            │                  │                  │
            ▼                  ▼                  ▼
┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐
│   Worker B        │  │   Worker C        │  │   Worker D        │
│   (Remote)        │  │   (Remote)        │  │   (Remote)        │
├───────────────────┤  ├───────────────────┤  ├───────────────────┤
│                   │  │                   │  │                   │
│ P2P Event Loop    │  │ P2P Event Loop    │  │ P2P Event Loop    │
│   ↓               │  │   ↓               │  │   ↓               │
│ req_res_handler   │  │ req_res_handler   │  │ req_res_handler   │
│   ↓               │  │   ↓               │  │   ↓               │
│ Auto send ACK:    │  │ Auto send ACK:    │  │ Auto send ACK:    │
│ GenericResponse { │  │ GenericResponse { │  │ GenericResponse { │
│   request_id,     │  │   request_id,     │  │   request_id,     │
│   success: true   │  │   success: true   │  │   success: true   │
│ }                 │  │ }                 │  │ }                 │
│   ↓               │  │   ↓               │  │   ↓               │
│ Process batch:    │  │ Process batch:    │  │ Process batch:    │
│ Processor         │  │ Processor         │  │ Processor         │
│   - Hash          │  │   - Hash          │  │   - Hash          │
│   - Store         │  │   - Store         │  │   - Store         │
│   - Report        │  │   - Report        │  │   - Report        │
└───────┬───────────┘  └───────┬───────────┘  └───────┬───────────┘
        │ ACK                  │ ACK                  │ ACK
        └──────────────────────┴──────────────────────┘
                               │
                               ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                              Worker A (Local)                               │
├────────────────────────────────────────────────────────────────────────────┤
│   ┌──────────────────────────────────────────────────────┐                │
│   │          P2P Event Loop (event_loop.rs)              │                │
│   │  - Nhận responses                                    │                │
│   └──────────────────────────┬───────────────────────────┘                │
│                              │ tx_req_res_event                            │
│                              ↓                                              │
│   ┌──────────────────────────────────────────────────────┐                │
│   │           P2P Event Router (CẦN IMPLEMENT)           │                │
│   │  match event:                                        │                │
│   │    ResponseReceived → tx_to_quorum                  │                │
│   │    RequestReceived → tx_to_processor                │                │
│   └──────┬───────────────────────────────────────────────┘                │
│          │ ReqResEvent::ResponseReceived                                  │
│          ↓                                                                  │
│   ┌──────────────────┐                                                     │
│   │  QuorumWaiter    │                                                     │
│   │  - Track ACKs    │                                                     │
│   │  - Count stake   │                                                     │
│   │  - Check quorum  │                                                     │
│   └──────┬───────────┘                                                     │
│          │ Khi stake >= quorum_threshold                                  │
│          ↓                                                                  │
│   ┌──────────────────┐                                                     │
│   │   Processor      │                                                     │
│   │   - Hash batch   │                                                     │
│   │   - Store        │                                                     │
│   │   - Send digest  │                                                     │
│   └──────┬───────────┘                                                     │
│          │ batch digest                                                    │
│          ↓                                                                  │
│   ┌──────────────────┐                                                     │
│   │  Primary         │                                                     │
│   │  (Consensus)     │                                                     │
│   └──────────────────┘                                                     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## ACK Tracking trong QuorumWaiter

```
QuorumWaiter State:
┌────────────────────────────────────────────────────────────���────┐
│ pending_batches: HashMap<batch_id, BatchInfo>                   │
│                                                                  │
│ BatchInfo {                                                      │
│   batch: Vec<u8>,                // Serialized batch data       │
│   expected_ids: HashSet<u64>,    // [101, 102, 103]            │
│   worker_names: Vec<PublicKey>,  // [Worker_B, Worker_C, ...]  │
│   received_acks: HashSet<u64>    // Initially empty             │
│ }                                                                │
└─────────────────────────────────────────────────────────────────┘

Timeline:
─────────────────────────────────────────────────────────────────►

t=0:  BatchMaker sends batch
      QuorumWaiter receives QuorumWaiterMessage {
          batch,
          request_ids: [101, 102, 103],
          worker_names: [B, C, D]
      }
      → Add to pending_batches

t=1:  ACK from Worker B arrives
      ReqResEvent::ResponseReceived { request_id: 101, ... }
      → received_acks.insert(101)
      → Calculate stake: self.stake + stake(B)
      → Check: stake < quorum_threshold → Continue waiting

t=2:  ACK from Worker C arrives
      ReqResEvent::ResponseReceived { request_id: 102, ... }
      → received_acks.insert(102)
      → Calculate stake: self.stake + stake(B) + stake(C)
      → Check: stake >= quorum_threshold ✅
      → Forward batch to Processor
      → Remove from pending_batches

t=3:  ACK from Worker D arrives (late)
      ReqResEvent::ResponseReceived { request_id: 103, ... }
      → Batch already processed, ignore
```

## Data Structures

### BatchMaker
```rust
struct BatchMaker {
    peer_mapping: HashMap<PublicKey, PeerId>,  // Worker name → P2P peer ID
    tx_req_res: Sender<ReqResCommand>,         // Send P2P commands
    request_counter: u64,                       // Unique request IDs
    current_batch: Batch,
    ...
}
```

### QuorumWaiter
```rust
struct QuorumWaiter {
    rx_req_res_event: Receiver<ReqResEvent>,   // Receive P2P ACKs
    pending_batches: HashMap<u64, (
        SerializedBatch,
        HashSet<u64>,         // expected request IDs
        Vec<PublicKey>,       // worker names for stake
        HashSet<u64>          // received ACKs
    )>,
    ...
}
```

### Worker
```rust
struct Worker {
    tx_req_res: Option<Sender<ReqResCommand>>,
    tx_req_res_event_to_quorum: Option<Sender<ReqResEvent>>,
    peer_mapping: HashMap<PublicKey, PeerId>,
    ...
}
```

## Message Flow

```
ReqResCommand::SendRequest
├─ request_id: u64           // Unique ID (counter)
├─ target_peer: PeerId       // From peer_mapping[worker.public_key]
└─ data: Vec<u8>             // Serialized WorkerMessage::Batch

          ↓ P2P Network ↓

GenericRequest
├─ request_id: u64           // Same as above
└─ data: Vec<u8>             // Batch data

          ↓ Automatic ACK ↓

GenericResponse
├─ request_id: u64           // Same request_id
├─ success: bool             // true
└─ message: String           // "ACK"

          ↓ Event Loop ↓

ReqResEvent::ResponseReceived
├─ request_id: u64
├─ from_peer: PeerId
├─ success: bool
└─ message: String
```

## Comparison: TCP vs P2P

| Feature | TCP (Old) | P2P (New) |
|---------|-----------|-----------|
| Protocol | Custom TCP | libp2p Request-Response |
| ACK Mechanism | CancelHandler futures | Automatic protocol ACK |
| Discovery | Static config (IP:Port) | mDNS + Kademlia DHT |
| NAT Traversal | ❌ Manual port forwarding | ✅ Hole punching, TURN |
| Connection Reuse | ❌ New connection per batch | ✅ Multiplexed streams |
| Retry Logic | Custom implementation | ✅ Built-in |
| Timeout | Custom timer | ✅ Configurable timeout |
| Message Size | Limited by TCP buffer | ✅ Streaming support |
