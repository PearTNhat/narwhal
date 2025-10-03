#!/bin/bash
# Script để test P2P demo với 2 node

# Kill các process node cũ nếu còn chạy
echo "🧹 Cleaning up old processes..."
pkill -f "node run.*primary" 2>/dev/null
sleep 1

echo "🚀 Building project..."
cargo build --release --bin node

if [ $? -ne 0 ]; then
    echo "❌ Build failed"
    exit 1
fi

echo "✅ Build successful"
echo ""
echo "🌐 Starting 2 primary nodes with P2P..."
echo ""

# Paths
KEYS_DIR="benchmark"
STORE_DIR="/home/abc/nhat/narwhal/benchmark/datanarwhal_p2p_demo"
LOGS_DIR="/home/abc/nhat/narwhal/benchmark/logs"

# Clean up old stores
rm -rf "${STORE_DIR}"
mkdir -p "${STORE_DIR}"

# Tạo thư mục log nếu chưa có
mkdir -p "${LOGS_DIR}"

# Get list of keys
KEYS=($(ls ${KEYS_DIR}/.node*.json 2>/dev/null | head -2))

if [ ${#KEYS[@]} -lt 2 ]; then
    echo "❌ Need at least 2 key files in ${KEYS_DIR}/"
    exit 1
fi

echo "📋 Found ${#KEYS[@]} keys"

# Start node 1
echo "▶️  Starting Node 1..."
./target/release/node run \
    --keys "${KEYS[0]}" \
    --committee "${KEYS_DIR}/.committee.json" \
    --store "${STORE_DIR}/node1" \
    primary > "${LOGS_DIR}/node1_p2p.log" 2>&1 &
NODE1_PID=$!

sleep 2

# Start node 2
echo "▶️  Starting Node 2..."
./target/release/node run \
    --keys "${KEYS[1]}" \
    --committee "${KEYS_DIR}/.committee.json" \
    --store "${STORE_DIR}/node2" \
    primary > "${LOGS_DIR}/node2_p2p.log" 2>&1 &
NODE2_PID=$!

echo ""
echo "✅ Nodes started!"
echo "   Node 1 PID: $NODE1_PID (log: ${LOGS_DIR}/node1_p2p.log)"
echo "   Node 2 PID: $NODE2_PID (log: ${LOGS_DIR}/node2_p2p.log)"
echo ""
echo "📊 Monitoring P2P messages (Ctrl+C to stop)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Monitor logs for P2P messages
tail -f "${LOGS_DIR}/node1_p2p.log" "${LOGS_DIR}/node2_p2p.log" | grep --line-buffered -E "P2P|Received P2P|Sending P2P|🎉|💬|📦|🌐|✅"

# Cleanup on exit
trap "echo ''; echo '🛑 Stopping nodes...'; kill $NODE1_PID $NODE2_PID 2>/dev/null; exit" INT TERM
