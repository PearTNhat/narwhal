// Copyright(C) Facebook, Inc. and its affiliates.
use anyhow::{Context, Result};
use clap::{crate_name, crate_version, App, AppSettings, ArgMatches, SubCommand};
use config::Export as _;
use config::Import as _;
use config::{Committee, KeyPair, Parameters, WorkerId};
use consensus::Consensus;
use env_logger::Env;
use libp2p::Multiaddr;
use log::info;
use primary::{Certificate, Primary};
use store::Store;
use tokio::sync::mpsc::{channel, Receiver};
use worker::{Worker, WorkerMessage};
// Thêm các use statements cần thiết
use bytes::{BufMut, Bytes, BytesMut};
use prost::Message;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::time::{sleep, Duration};

// Import P2P modules
use libp2p::{gossipsub, PeerId};
use p2p::{
    event_loop,
    functions::{create_derived_keypair},
    types::{get_primary_topic, get_worker_topic, P2pMessage, PrimaryMessage1, WorkerMessage1},
    ReqResCommand, ReqResEvent,
};

// Thêm module để import các struct được tạo bởi prost
pub mod comm {
    include!(concat!(env!("OUT_DIR"), "/comm.rs"));
}

/// The default channel capacity.
pub const CHANNEL_CAPACITY: usize = 1_000;

#[tokio::main]
async fn main() -> Result<()> {
    let matches = App::new(crate_name!()) //// Định nghĩa các lệnh và tham số
        .version(crate_version!())
        .about("A research implementation of Narwhal and Tusk.")
        .args_from_usage("-v... 'Sets the level of verbosity'")
        .subcommand(
            SubCommand::with_name("generate_keys")
                .about("Print a fresh key pair to file")
                .args_from_usage("--filename=<FILE> 'The file where to print the new key pair'"),
        )
        .subcommand(
            SubCommand::with_name("run")
                .about("Run a node")
                .args_from_usage("--keys=<FILE> 'The file containing the node keys'")
                .args_from_usage("--committee=<FILE> 'The file containing committee information'")
                .args_from_usage("--parameters=[FILE] 'The file containing the node parameters'")
                .args_from_usage("--store=<PATH> 'The path where to create the data store'")
                .subcommand(SubCommand::with_name("primary").about("Run a single primary"))
                .subcommand(
                    SubCommand::with_name("worker")
                        .about("Run a single worker")
                        .args_from_usage("--id=<INT> 'The worker id'"),
                )
                .setting(AppSettings::SubcommandRequiredElseHelp),
        )
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .get_matches();
    // ... Thiết lập logging ...
    let log_level = match matches.occurrences_of("v") {
        0 => "error",
        1 => "warn",
        2 => "info",
        3 => "debug",
        _ => "trace",
    };
    let mut logger = env_logger::Builder::from_env(Env::default().default_filter_or(log_level));
    #[cfg(feature = "benchmark")]
    logger.format_timestamp_millis();
    logger.init();

    match matches.subcommand() {
        ("generate_keys", Some(sub_matches)) => KeyPair::new()
            .export(sub_matches.value_of("filename").unwrap())
            .context("Failed to generate key pair")?,
        ("run", Some(sub_matches)) => run(sub_matches).await?,
        _ => unreachable!(),
    }
    Ok(())
}

// Runs either a worker or a primary.
async fn run(matches: &ArgMatches<'_>) -> Result<()> {
    // 1. Đọc các đường dẫn file từ tham số
    let key_file = matches.value_of("keys").unwrap();
    let committee_file = matches.value_of("committee").unwrap();
    let parameters_file = matches.value_of("parameters");
    let store_path = matches.value_of("store").unwrap();
    // 2. Tải cấu hình từ các file JSON
    // Read the committee and node's keypair from file.
    let keypair = KeyPair::import(key_file).context("Failed to load the node's keypair")?;

    let committee =
        Committee::import(committee_file).context("Failed to load the committee information")?;

    // Load default parameters if none are specified.
    let parameters = match parameters_file {
        Some(filename) => {
            Parameters::import(filename).context("Failed to load the node's parameters")?
        }
        None => Parameters::default(),
    };
    // Make the data store.
    let store = Store::new(store_path).context("Failed to create a store")?;

    // Channels the sequence of certificates.
    let (tx_output, rx_output) = channel(CHANNEL_CAPACITY);

    let role: &str;
    // 2. XÁC ĐỊNH VAI TRÒ VÀ TẠO P2P KEYPAIR DUY NHẤT
    // Logic này được chuyển lên trước để quyết định component_id
    let (p2p_keypair, _) = match matches.subcommand() {
        ("primary", _) => {
            role = "Primary";
            info!("Running as a PRIMARY");
            // Sử dụng một ID đặc biệt cho Primary để đảm bảo nó khác với mọi Worker
            let key = create_derived_keypair(&keypair.name, u32::MAX);
            (key, "Primary")
        }
        ("worker", Some(sub_matches)) => {
            role = "Worker";
            let id = sub_matches.value_of("id").unwrap().parse::<WorkerId>()?;
            info!("Running as a WORKER with id {}", id);
            // Sử dụng chính WorkerId để tạo keypair duy nhất
            let key = create_derived_keypair(&keypair.name, id);
            (key, "Worker")
        }
        _ => unreachable!(),
    }; 

    let local_peer_id = p2p_keypair.public().to_peer_id();
    info!("📌 P2P Peer ID for {}: {}", role, local_peer_id);

    // Tạo Swarm
    let mut swarm = p2p::create_p2p_swarm(p2p_keypair, local_peer_id)
        .await
        .expect("Failed to create P2P swarm");


    // Lắng nghe trên một port duy nhất (logic tìm port trống của bạn rất tốt)
    let mut p2p_port = 9000; // Port khởi đầu
    for _ in 0..15 {
        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/udp/{}/quic-v1", p2p_port).parse()?;
        match swarm.listen_on(listen_addr) {
            Ok(_) => {
                info!("✅ P2P listening on port {}", p2p_port);
                break;
            }
            Err(e) => {
                info!("⚠️ Port {} in use, trying next. Error: {}", p2p_port, e);
                p2p_port += 1;
            }
        }
    }
    // Đăng ký (subscribe) vào các topic phù hợp với vai trò của node
    let primary_topic = get_primary_topic();
    let worker_topic = get_worker_topic();
    if matches.subcommand_matches("primary").is_some() {
        swarm.behaviour_mut().gossipsub.subscribe(&primary_topic)?;
        info!("P ___Subscribed to PRIMARY topic: {}", primary_topic);
    }
    if matches.subcommand_matches("worker").is_some() {
        swarm.behaviour_mut().gossipsub.subscribe(&worker_topic)?;
        info!("W ___Subscribed to WORKER topic: {}", worker_topic);
    }
    // --- 3. TẠO CÁC KÊNH GIAO TIẾP CÓ CẤU TRÚC ---
    // Kênh để logic chính gửi message ra P2P event loop
    let (tx_to_p2p, rx_from_core) = channel(CHANNEL_CAPACITY);
    // Kênh để P2P event loop gửi message đã xử lý vào logic Primary
    let (tx_to_primary, mut rx_for_primary) = channel(CHANNEL_CAPACITY);
    // Kênh để P2P event loop gửi message đã xử lý vào logic Worker
    let (tx_to_worker, mut rx_for_worker) = channel(CHANNEL_CAPACITY);
    
    // Kênh cho Request-Response
    let (tx_req_res_command, rx_req_res_command) = channel(CHANNEL_CAPACITY);
    let (tx_req_res_event, mut rx_req_res_event) = channel(CHANNEL_CAPACITY);
    
    // Chạy P2P event loop trong một task riêng
    tokio::spawn(async move {
        event_loop::run_p2p_event_loop(
            swarm, 
            rx_from_core, 
            tx_to_primary, 
            tx_to_worker,
            rx_req_res_command,
            tx_req_res_event,
        ).await;
    });

    // --- KẾT THÚC KHỞI TẠO P2P ---
    
    // --- 4A. DEMO GOSSIPSUB (Hello messages) ---
    let tx_p2p_clone = tx_to_p2p.clone();
    let key_name_clone = keypair.name.clone();
    tokio::spawn(async move {
        use tokio::time::{sleep, Duration};
        sleep(Duration::from_secs(5)).await; // Đợi mạng ổn định
        let mut counter: u64 = 0;
        loop {
           counter += 1;
           
           // Chỉ gửi Hello message qua gossipsub
           let hello_msg = format!(
               "Hello from {} {}: [Seq: {}]",
               role, key_name_clone, counter
           );

           let (topic, p2p_message) = if role == "Primary" {
               (get_primary_topic(), P2pMessage::Primary(PrimaryMessage1::Hello(hello_msg)))
           } else {
               (get_worker_topic(), P2pMessage::Worker(WorkerMessage1::Hello(hello_msg)))
           };
           
        //    log::info!("📤 [GOSSIPSUB] Sending Hello message #{}", counter);
        //    if tx_p2p_clone.send((topic, p2p_message)).await.is_err() {
        //        log::warn!("Core logic channel closed, stopping demo sender.");
        //        break;
        //    }
           
           sleep(Duration::from_secs(10)).await;
        }
    });
    
    // --- 4B. DEMO REQUEST-RESPONSE (Với ACK tracking) ---
    let tx_req_res_cmd_clone = tx_req_res_command.clone();
    let key_name_clone2 = keypair.name.clone();
    let local_peer_id_clone = local_peer_id;
    tokio::spawn(async move {
        sleep(Duration::from_secs(8)).await; // Đợi mạng ổn định và peer discovery
        
        // Lấy danh sách peers đã discovered (sẽ được populate bởi mDNS)
        // Trong production, bạn sẽ track peers qua mDNS events
        let mut request_counter: u64 = 0;
        
        loop {
            request_counter += 1;
            let discovered_peer_id = local_peer_id_clone; // Giả sử chúng ta có một peer khác để gửi request
            // Trong thực tế, bạn sẽ maintain một list peers từ mDNS/Kad
            log::info!("📤 [REQ-RES DEMO] Would send Request #{} if peers available", request_counter);
            log::info!("    💡 To demo: Run another node, it will auto-discover via mDNS");
            log::info!("    💡 Requests will be sent automatically to discovered peers");
            
          //  TODO: Khi có peer, gửi request như sau:
            let data = format!("Request data from {}", key_name_clone2);
            if let Err(e) = tx_req_res_cmd_clone.send(ReqResCommand::SendRequest {
                request_id: request_counter,
                target_peer: discovered_peer_id,
                data: data.into_bytes(),
            }).await {
                log::warn!("Failed to send ReqResCommand: {}", e);
            }
            
            sleep(Duration::from_secs(15)).await;
        }
    });
    
    // --- 4C. Task lắng nghe Request-Response Events (ACKs) ---
    tokio::spawn(async move {
        use std::collections::HashMap;
        let mut pending_requests: HashMap<u64, tokio::time::Instant> = HashMap::new();
        
        info!("👂 [REQ-RES] Event listener started");
        
        while let Some(event) = rx_req_res_event.recv().await {
            match event {
                ReqResEvent::ResponseReceived { request_id, from_peer, success, message } => {
                    if let Some(sent_time) = pending_requests.remove(&request_id) {
                        let latency = sent_time.elapsed();
                        info!("✅ [REQ-RES] ACK received for Request #{}", request_id);
                        info!("    ├─ From: {}", from_peer);
                        info!("    ├─ Success: {}", success);
                        info!("    ├─ Message: '{}'", message);
                        info!("    └─ Latency: {:?}", latency);
                    }
                }
                ReqResEvent::RequestFailed { request_id, to_peer, error } => {
                    pending_requests.remove(&request_id);
                    log::warn!("❌ [REQ-RES] Request #{} failed to {}: {}", request_id, to_peer, error);
                }
                ReqResEvent::RequestReceived { request_id, from_peer, data } => {
                    info!("📨 [REQ-RES] Received Request #{} from {}", request_id, from_peer);
                    info!("    └─ Data: {} bytes", data.len());
                    // ACK đã được tự động gửi trong handler
                }
            }
        }
    });

    // Task lắng nghe message cho Primary
    tokio::spawn(async move {
        info!("👂 PRIMARY message listener task started.");
        while let Some(message) = rx_for_primary.recv().await {
            match message {
                PrimaryMessage1::Hello(msg) => {
                    info!("🎉 [PRIMARY LOGIC] Received Hello: '{}'", msg);
                }
                PrimaryMessage1::Request { request_id, data } => {
                    info!("📨 [PRIMARY LOGIC] Received Request #{}: {}", request_id, data);
                    // Logic xử lý request ở đây (response đã tự động gửi trong handler)
                }
                PrimaryMessage1::Response { request_id, data } => {
                    info!("✅ [PRIMARY LOGIC] Received Response #{}: {}", request_id, data);
                    // Xử lý response ở đây
                }
                // Thêm các case xử lý cho các loại message khác của Primary
                // PrimaryMessage::Header(header_bytes) => { ... }
            }
        }
    });

    // Task lắng nghe message cho Worker
    tokio::spawn(async move {
        info!("👂 WORKER message listener task started.");
        while let Some(message) = rx_for_worker.recv().await {
             match message {
                WorkerMessage1::Hello(msg) => {
                     info!("🎉 [WORKER LOGIC] Received Hello: '{}'", msg);
                }
                WorkerMessage1::Request { request_id, data } => {
                    info!("📨 [WORKER LOGIC] Received Request #{}: {}", request_id, data);
                    // Logic xử lý request ở đây (response đã tự động gửi trong handler)
                }
                WorkerMessage1::Response { request_id, data } => {
                    info!("✅ [WORKER LOGIC] Received Response #{}: {}", request_id, data);
                    // Xử lý response ở đây
                }
                // Thêm các case xử lý cho các loại message khác của Worker
             }
        }
    });

    // Check whether to run a primary, a worker, or an entire authority.
    match matches.subcommand() {
        // Spawn the primary and consensus core.
        // Nó khởi chạy cả Primary (đề xuất các khối/header) và Consensus (sắp xếp các khối/header đó).
        // Hai thành phần này giao tiếp với nhau qua các kênh (channels).
        // Quan trọng nhất, kết quả cuối cùng của quá trình đồng thuận (một dòng các Certificate đã được sắp xếp) được gửi qua kênh tx_output.
        // Sau đó, nó gọi hàm analyze(rx_output, ...) để xử lý dòng Certificate này.
        ("primary", _) => {
            // Xác định ID duy nhất cho node này dựa trên vị trí public key trong committee.
            let mut primary_keys: Vec<_> = committee.authorities.keys().cloned().collect();
            primary_keys.sort(); // Sắp xếp để đảm bảo thứ tự là nhất quán trên tất cả các node.

            let node_id = primary_keys
                .iter()
                .position(|pk| pk == &keypair.name)
                .unwrap();
            log::info!("Node {} khởi chạy với ID: {}", keypair.name, node_id);
            //Đề xuât cho Người điều phối (Consensus)
            let (tx_new_certificates, rx_new_certificates) = channel(CHANNEL_CAPACITY);
            // kênh nhận phản hồi từ Người điều phối (Consensus)
            let (tx_feedback, rx_feedback) = channel(CHANNEL_CAPACITY);
            // Bắt đầu công việc của Bếp trưởng
            Primary::spawn(
                keypair,
                committee.clone(),
                parameters.clone(),
                store.clone(), // Clone the store for the primary
                /* tx_consensus */ tx_new_certificates,
                /* rx_consensus */ rx_feedback,
            );
            // Bắt đầu công việc của Người điều phối
            Consensus::spawn(
                committee,
                parameters.gc_depth,
                store.clone(),
                rx_new_certificates,
                tx_feedback,
                tx_output,
            );
            // Giao kết quả cuối cùng cho người phục vụ
            analyze(rx_output, node_id, store).await;
        }
        //Chỉ đơn giản là khởi chạy một Worker. Worker chịu trách nhiệm nhận giao dịch từ client và tạo các batch (lô) giao dịch.
        // Spawn a single worker.
        ("worker", Some(sub_matches)) => {
            let id = sub_matches
                .value_of("id")
                .unwrap()
                .parse::<WorkerId>()
                .context("The worker id must be a positive integer")?;
            
            // TODO: Tạo peer_mapping từ committee (map PublicKey -> PeerId)
            // Hiện tại dùng HashMap rỗng, cần implement peer discovery sau
            let peer_mapping = std::collections::HashMap::new();
            
            Worker::spawn(
                keypair.name, 
                id, 
                committee, 
                parameters, 
                store,
                Some(tx_req_res_command.clone()),
                None, // tx_req_res_event_to_quorum sẽ được tạo bên trong Worker
                peer_mapping,
            );
        }
        _ => unreachable!(),
    }

    // Giữ cho chương trình chính không bị thoát, cho phép các task con (primary/worker) tiếp tục chạy.
    // Chúng ta tạo một kênh mới và chờ mãi mãi ở đầu nhận.
    let (_tx, mut rx) = channel::<()>(1);
    let _ = rx.recv().await;

    unreachable!();
}

//Cầu nối đến Lớp Thực thi (Executor)
/// Receives an ordered list of certificates and apply any application-specific logic.
async fn analyze(mut rx_output: Receiver<Certificate>, node_id: usize, mut store: Store) {
    // SỬA LỖI: Thêm `mut` vào đây
    // Helper function for varint encoding (no changes needed here)
    fn put_uvarint_to_bytes_mut(buf: &mut BytesMut, mut value: u64) {
        loop {
            if value < 0x80 {
                buf.put_u8(value as u8);
                break;
            }
            buf.put_u8(((value & 0x7F) | 0x80) as u8);
            value >>= 7;
        }
    }
    // 1. Kết nối tới Executor qua Unix Socket
    let socket_path = format!("/tmp/executor{}.sock", node_id);
    log::info!(
        "[ANALYZE] Node ID {} attempting to connect to {}",
        node_id,
        socket_path
    );
    //// Vòng lặp kết nối lại nếu thất bại
    let mut stream = loop {
        match UnixStream::connect(&socket_path).await {
            Ok(stream) => {
                log::info!(
                    "[ANALYZE] Node ID {} connected successfully to {}",
                    node_id,
                    socket_path
                );
                break stream;
            }
            Err(e) => {
                log::warn!(
                    "[ANALYZE] Node ID {}: Connection to {} failed: {}. Retrying...",
                    node_id,
                    socket_path,
                    e
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    };

    log::info!(
        "[ANALYZE] Node ID {} entering loop to wait for committed blocks.",
        node_id
    );
    // 2. Vòng lặp chính: chờ kết quả từ Consensus
    while let Some(certificate) = rx_output.recv().await {
        log::info!(
            "[ANALYZE] Node ID {} RECEIVED certificate for round {} from consensus.",
            node_id,
            certificate.header.round
        );

        let mut all_transactions = Vec::new();

        for (digest, worker_id) in certificate.header.payload {
            // Đọc batch từ store bằng digest.
            match store.read(digest.to_vec()).await {
                Ok(Some(serialized_batch_message)) => {
                    // Giải mã `WorkerMessage::Batch`
                    match bincode::deserialize(&serialized_batch_message) {
                        Ok(WorkerMessage::Batch(batch)) => {
                            log::debug!(
                                "[ANALYZE] Unpacked batch {} with {} transactions for worker {}.",
                                digest,
                                batch.len(),
                                worker_id
                            );
                            // Chuyển đổi mỗi giao dịch trong batch sang định dạng protobuf.
                            for tx_data in batch {
                                all_transactions.push(comm::Transaction {
                                    // Trường 'digest' trong protobuf giờ sẽ chứa toàn bộ dữ liệu giao dịch.
                                    digest: tx_data,
                                    worker_id: worker_id as u32,
                                });
                            }
                        }
                        Ok(_) => {
                            log::warn!(
                                "[ANALYZE] Digest {} did not correspond to a Batch message.",
                                digest
                            );
                        }
                        Err(e) => {
                            log::error!(
                                "[ANALYZE] Failed to deserialize message for digest {}: {}",
                                digest,
                                e
                            );
                        }
                    }
                }
                Ok(None) => {
                    log::warn!("[ANALYZE] Batch for digest {} not found in store.", digest);
                }
                Err(e) => {
                    log::error!(
                        "[ANALYZE] Failed to read batch for digest {}: {}",
                        digest,
                        e
                    );
                }
            }
        }

        // Bỏ qua việc gửi block nếu không có giao dịch nào được tìm thấy
        if all_transactions.is_empty() {
            log::warn!(
                "[ANALYZE] No transactions found for certificate in round {}. Skipping.",
                certificate.header.round
            );
            continue;
        }

        let committed_block = comm::CommittedBlock {
            epoch: certificate.header.round,
            height: certificate.header.round,
            transactions: all_transactions, // Sử dụng danh sách đầy đủ các giao dịch
        };

        let epoch_data = comm::CommittedEpochData {
            blocks: vec![committed_block],
        };

        log::debug!(
            "[ANALYZE] Node ID {} serializing data for round {}",
            node_id,
            certificate.header.round
        );
        let mut proto_buf = BytesMut::new();
        epoch_data
            .encode(&mut proto_buf)
            .expect("FATAL: Protobuf serialization failed!");

        let mut len_buf = BytesMut::new();
        put_uvarint_to_bytes_mut(&mut len_buf, proto_buf.len() as u64);

        log::info!("[ANALYZE] Node ID {} WRITING {} bytes (len) and {} bytes (data) to socket for round {}.", node_id, len_buf.len(), proto_buf.len(), certificate.header.round);

        if let Err(e) = stream.write_all(&len_buf).await {
            log::error!(
                "[ANALYZE] FATAL: Node ID {}: Failed to write length to socket: {}",
                node_id,
                e
            );
            break;
        }

        if let Err(e) = stream.write_all(&proto_buf).await {
            log::error!(
                "[ANALYZE] FATAL: Node ID {}: Failed to write payload to socket: {}",
                node_id,
                e
            );
            break;
        }

        log::info!(
            "[ANALYZE] SUCCESS: Node ID {} sent block for round {} successfully.",
            node_id,
            certificate.header.round
        );
    }

    log::warn!(
        "[ANALYZE] Node ID {} exited the receive loop. No more blocks will be processed.",
        node_id
    );
}
