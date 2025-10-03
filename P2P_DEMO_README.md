# Demo P2P cho Narwhal

## Tổng quan

Demo này tích hợp lớp mạng **libp2p** song song với lớp mạng **TCP** hiện tại của Narwhal. Các node sẽ gửi/nhận tin nhắn "Hello" qua gossipsub để kiểm tra kết nối P2P.

### Điểm quan trọng:
- ✅ **TCP vẫn hoạt động bình thường** - Primary và Worker vẫn dùng TCP cho tất cả các giao tiếp chính
- ✅ **P2P chạy song song** - Chỉ để test, không ảnh hưởng đến logic hiện tại
- ✅ **Dễ dàng tắt/bật** - Có thể remove code P2P mà không ảnh hưởng hệ thống

## Kiến trúc

```
┌─────────────────────────────────────────────────┐
│                   Node                          │
│                                                 │
│  ┌──────────────┐         ┌─────────────────┐  │
│  │   Primary    │◄────TCP──►│    Worker     │  │
│  │   (TCP)      │         │    (TCP)       │  │
│  └──────────────┘         └─────────────────┘  │
│         │                                       │
│         │ (demo only)                           │
│         ▼                                       │
│  ┌──────────────────────────────────────┐      │
│  │      P2P Layer (libp2p)              │      │
│  │  • Gossipsub                         │      │
│  │  • mDNS (local discovery)            │      │
│  │  • Kademlia DHT                      │      │
│  └──────────────────────────────────────┘      │
│         │                                       │
└─────────┼───────────────────────────────────────┘
          │
          ▼ gossipsub
    ┌──────────────┐
    │  Other Nodes │
    └──────────────┘
```

## Cấu trúc Code

### Crate `p2p/` (mới)
```
p2p/
├── src/
│   ├── event_loop.rs    # Vòng lặp xử lý sự kiện P2P
│   ├── functions.rs     # Tạo swarm, keypair
│   ├── handlers.rs      # Xử lý các sự kiện libp2p
│   ├── receiver.rs      # P2PReceiver (tương tự network::Receiver)
│   ├── sender.rs        # P2PSender (tương tự network::SimpleSender)
│   ├── state.rs         # State đơn giản
│   ├── types.rs         # Định nghĩa MyBehaviour, message types
│   └── lib.rs
└── Cargo.toml
```

### Thay đổi trong `node/src/main.rs`

1. **Khởi tạo P2P** (dòng ~110):
   - Tạo libp2p keypair từ keypair của Narwhal
   - Tạo Swarm với Gossipsub + mDNS + Kademlia
   - Listen trên port = TCP port + 1000
   - Subscribe vào topic "narwhal-demo-hello"

2. **P2P Event Loop** (task riêng):
   - Nhận message từ gossipsub → gửi vào kênh `tx_to_primary`
   - Nhận từ kênh `rx_from_primary` → publish lên gossipsub

3. **Demo Tasks**:
   - **Sender**: Gửi "Hello from node: XXX" mỗi 10 giây
   - **Listener**: In ra message nhận được từ các node khác

## Chạy Demo

### Bước 1: Build project
```bash
cd /home/abc/nhat/narwhal
cargo build --release --bin node
```

### Bước 2: Chạy script demo
```bash
./test_p2p_demo.sh
```

Script sẽ:
1. Khởi động 2 primary node
2. Mỗi node lắng nghe trên:
   - TCP: port từ `.committee.json` (4000, 4005, ...)
   - P2P: TCP port + 1000 (5000, 5005, ...)
3. Hiển thị log real-time của các P2P message

### Bước 3: Quan sát output

Bạn sẽ thấy:
```
🌐 P2P Node listening on: /ip4/127.0.0.1/udp/5000/quic-v1/p2p/12D3...
✅ P2P Connection established with peer: 12D3...
💬 Sending P2P demo message: Hello from node: 8TwN6ZRT
📦 P2P Receiver got 28 bytes from Swarm
🎉 Received P2P message: Hello from node: MnsTBD8M
```

## Kiểm tra Port

```bash
# Xem các port đang mở
sudo netstat -tulpn | grep node

# TCP (primary-to-primary):
tcp   0.0.0.0:4000   node
tcp   0.0.0.0:4005   node

# UDP (P2P):
udp   0.0.0.0:5000   node
udp   0.0.0.0:5005   node
```

## Debug

### Xem log chi tiết
```bash
# Node 1
tail -f /tmp/node1_p2p.log

# Node 2  
tail -f /tmp/node2_p2p.log

# Chỉ xem P2P messages
tail -f /tmp/node*.log | grep "P2P\|🎉\|💬"
```

### Kiểm tra kết nối
```bash
# Kiểm tra process
ps aux | grep node

# Kill tất cả
pkill -f "node run"
```

## Tích hợp vào Primary/Worker (Tương lai)

Khi P2P đã ổn định, bạn có thể:

1. **Thay thế `NetworkReceiver`** trong `Primary`:
   ```rust
   // Thay vì
   NetworkReceiver::spawn(address, handler);
   
   // Dùng
   P2PReceiver::spawn(rx_from_swarm, handler);
   ```

2. **Thay thế `SimpleSender`** trong `Core`:
   ```rust
   // Thay vì
   let mut sender = SimpleSender::new();
   sender.send(address, message).await;
   
   // Dùng
   let sender = P2PSender::new(tx_to_swarm);
   sender.send(&message).await; // Broadcast, không cần address
   ```

3. **Bootstrap nodes**:
   - Thêm địa chỉ P2P vào config
   - Gọi `bootstrap_network()` khi khởi động

## Dọn dẹp

Để remove toàn bộ P2P và quay lại TCP thuần:

1. Xóa crate `p2p/`
2. Remove import P2P trong `node/src/main.rs`
3. Xóa phần "KHỞI TẠO P2P" và "DEMO P2P"
4. Xóa dependency `p2p` trong `node/Cargo.toml`

## Lợi ích của P2P (so với TCP)

| Tính năng | TCP (hiện tại) | P2P (libp2p) |
|-----------|----------------|--------------|
| **NAT Traversal** | ❌ Cần public IP | ✅ Tự động (STUN/TURN) |
| **Peer Discovery** | ❌ Phải config thủ công | ✅ mDNS + Kademlia DHT |
| **Encryption** | ⚠️ Tùy chọn | ✅ Mặc định (Noise/TLS) |
| **Multi-path** | ❌ 1 kết nối | ✅ Nhiều transport (QUIC, TCP, WebSocket) |
| **Broadcast** | ❌ Phải gửi từng peer | ✅ Gossipsub tối ưu |
| **Resilience** | ⚠️ Nếu 1 node chết, mất kết nối | ✅ Tự động reconnect |

## Câu hỏi thường gặp

**Q: P2P có làm chậm hệ thống không?**
A: Trong demo này không, vì P2P chỉ chạy song song để test. Khi tích hợp thật, cần benchmark.

**Q: Tại sao port P2P = port TCP + 1000?**
A: Để tránh conflict và dễ nhớ. Bạn có thể đổi trong code.

**Q: Có thể chạy trên nhiều máy không?**
A: Có! Thay `127.0.0.1` trong `.committee.json` bằng IP thật, và config firewall mở port UDP.

**Q: mDNS không work qua Internet?**
A: Đúng, mDNS chỉ cho LAN. Để work qua Internet, dùng bootstrap nodes hoặc relay.

## Next Steps

1. ✅ **Demo cơ bản** (đã xong)
2. 🔄 Test với 4 nodes
3. 🔄 Tích hợp P2PSender vào Core  
4. 🔄 Thay thế NetworkReceiver hoàn toàn
5. 🔄 Benchmark performance
6. 🔄 Test trên nhiều máy

---

**Tác giả**: GitHub Copilot  
**Ngày**: 2025-10-03
