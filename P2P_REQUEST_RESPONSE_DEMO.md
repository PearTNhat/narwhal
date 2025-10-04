# Demo Request-Response trong libp2p Gossipsub

## Tổng quan

Hệ thống này demo cách gửi tin nhắn Request và nhận lại Response tự động thông qua libp2p gossipsub.

## Cơ chế hoạt động

### 1. **Cấu trúc Message**

Trong `p2p/src/types.rs`, chúng ta đã định nghĩa 2 loại message mới:

```rust
pub enum PrimaryMessage1 {
    Hello(String),
    Request { request_id: u64, data: String },   // Tin nhắn yêu cầu
    Response { request_id: u64, data: String },  // Tin nhắn phản hồi
}

pub enum WorkerMessage1 {
    Hello(String),
    Request { request_id: u64, data: String },
    Response { request_id: u64, data: String },
}
```

### 2. **Auto-Response trong Handler**

Trong `p2p/src/handlers.rs`, hàm `handle_gossipsub_event()` đã được cập nhật để:

- **Khi nhận Request**: Tự động tạo và gửi Response ngay lập tức
- **Khi nhận Response**: Chuyển tiếp đến logic xử lý

```rust
// Ví dụ với Primary
if let PrimaryMessage1::Request { request_id, ref data } = primary_msg {
    info!("📨 [PRIMARY] Received Request #{}: {}", request_id, data);
    
    // Tạo và gửi response tự động
    let response = PrimaryMessage1::Response {
        request_id,
        data: format!("Response to: {}", data),
    };
    
    // Publish response lên gossipsub
    swarm.behaviour_mut().gossipsub.publish(topic, serialized_response);
}
```

### 3. **Demo trong Main**

Trong `node/src/main.rs`, demo task sẽ:

- **Chẵn (counter % 2 == 0)**: Gửi Request message
- **Lẻ**: Gửi Hello message bình thường

```rust
if counter % 2 == 0 {
    // Gửi Request
    P2pMessage::Primary(PrimaryMessage1::Request {
        request_id: counter,
        data: request_data,
    })
} else {
    // Gửi Hello
    P2pMessage::Primary(PrimaryMessage1::Hello(hello_msg))
}
```

### 4. **Xử lý trong Logic Layer**

Có 2 task riêng biệt để xử lý message:

**Primary Logic:**
```rust
tokio::spawn(async move {
    while let Some(message) = rx_for_primary.recv().await {
        match message {
            PrimaryMessage1::Request { request_id, data } => {
                info!("📨 [PRIMARY LOGIC] Received Request #{}", request_id);
            }
            PrimaryMessage1::Response { request_id, data } => {
                info!("✅ [PRIMARY LOGIC] Received Response #{}", request_id);
            }
            // ... other cases
        }
    }
});
```

**Worker Logic:** Tương tự

## Luồng hoạt động

```
┌─────────────┐         ┌──────────────┐         ┌─────────────┐
│   Node A    │         │   Gossipsub  │         │   Node B    │
│  (Primary)  │         │   Network    │         │  (Primary)  │
└──────┬──────┘         └──────┬───────┘         └──────┬──────┘
       │                       │                        │
       │ 1. Send Request #2    │                        │
       ├──────────────────────>│                        │
       │                       │  2. Broadcast Request  │
       │                       ├───────────────────────>│
       │                       │                        │
       │                       │  3. Auto-create Response
       │                       │                        ├─┐
       │                       │                        │ │
       │                       │                        │<┘
       │                       │  4. Send Response #2   │
       │  5. Receive Response  │<───────────────────────┤
       │<──────────────────────┤                        │
       │                       │                        │
       │ 6. Log in PRIMARY LOGIC                        │
       ├─┐                     │                        │
       │ │ ✅ Response #2      │                        │
       │<┘                     │                        │
       │                       │                        │
```

## Log Output mẫu

Khi chạy, bạn sẽ thấy các log như sau:

```
[INFO] 📤 [DEMO] Sending Request #2: Request from Primary node1
[INFO] ✅ Published message to topic narwhal-primary-consensus
[INFO] 📨 [PRIMARY] Received Request #2: Request from Primary node1
[INFO] ✅ [PRIMARY] Sent Response #2
[INFO] 📨 [PRIMARY LOGIC] Received Request #2: Request from Primary node1
[INFO] ✅ [PRIMARY LOGIC] Received Response #2: Response to: Request from Primary node1
```

## Chạy Demo

### Bước 1: Build project
```bash
cd /home/abc/nhat/narwhal
cargo build
```

### Bước 2: Chạy Primary node đầu tiên
```bash
./target/debug/node run \
  --keys committee.json \
  --committee committee.json \
  --parameters parameters.json \
  --store db_primary_0 \
  primary
```

### Bước 3: Chạy Primary node thứ hai (terminal khác)
```bash
./target/debug/node run \
  --keys committee.json \
  --committee committee.json \
  --parameters parameters.json \
  --store db_primary_1 \
  primary
```

### Bước 4: Quan sát logs

Bạn sẽ thấy:
- Node A gửi Request
- Node B nhận Request và tự động gửi Response
- Node A nhận được Response và log ra

## Mở rộng

### Thêm logic xử lý Request phức tạp hơn

Trong `p2p/src/handlers.rs`, bạn có thể thay đổi logic tạo response:

```rust
if let PrimaryMessage1::Request { request_id, ref data } = primary_msg {
    // Xử lý request phức tạp hơn
    let processed_data = process_request(data);
    
    let response = PrimaryMessage1::Response {
        request_id,
        data: processed_data,
    };
    // ... send response
}
```

### Track pending requests

Bạn có thể thêm một HashMap để track các request đang chờ response:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

type PendingRequests = Arc<Mutex<HashMap<u64, tokio::time::Instant>>>;

// Khi gửi request
pending_requests.lock().await.insert(request_id, Instant::now());

// Khi nhận response
if let Some(sent_time) = pending_requests.lock().await.remove(&request_id) {
    let latency = sent_time.elapsed();
    info!("Request #{} completed in {:?}", request_id, latency);
}
```

## Ưu điểm của cách này

1. **Tự động hóa**: Response được gửi tự động trong event handler
2. **Đơn giản**: Không cần quản lý channels phức tạp
3. **Gossipsub native**: Sử dụng tính năng broadcast của gossipsub
4. **Scalable**: Tất cả nodes nhận request đều có thể gửi response

## Hạn chế

1. **Broadcast**: Tất cả nodes trên topic đều nhận request và gửi response (có thể tốn bandwidth)
2. **Không đảm bảo thứ tự**: Response có thể đến theo thứ tự khác với request
3. **Không có timeout**: Cần thêm logic để handle request timeout

## Cải tiến tiếp theo

Để request-response hiệu quả hơn, bạn có thể:

1. Sử dụng libp2p's `request-response` protocol thay vì gossipsub
2. Thêm target peer_id vào Request để chỉ định node cụ thể
3. Implement timeout mechanism
4. Track và retry failed requests
