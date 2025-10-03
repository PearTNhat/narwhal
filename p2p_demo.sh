#!/bin/bash
# Helper script để quản lý P2P demo nodes

case "$1" in
    start)
        echo "🚀 Starting P2P demo..."
        ./test_p2p_demo.sh
        ;;
    
    stop)
        echo "🛑 Stopping all nodes..."
        pkill -f "node run.*primary"
        echo "✅ All nodes stopped"
        ;;
    
    logs)
        if [ -z "$2" ]; then
            echo "📋 Showing logs from all nodes..."
            tail -f benchmark/logs/node*_p2p.log
        else
            echo "📋 Showing logs from node $2..."
            tail -f "benchmark/logs/node${2}_p2p.log"
        fi
        ;;
    
    p2p)
        echo "📊 Filtering P2P messages only..."
        tail -f benchmark/logs/node*_p2p.log | grep --line-buffered -E "P2P|🎉|💬|📦|🌐|✅"
        ;;
    
    clean)
        echo "🧹 Cleaning up..."
        pkill -f "node run.*primary" 2>/dev/null
        rm -rf /tmp/narwhal_p2p_demo
        rm -f benchmark/logs/node*_p2p.log
        echo "✅ Cleanup complete"
        ;;
    
    status)
        echo "📊 Node Status:"
        echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
        ps aux | grep "node run.*primary" | grep -v grep | while read line; do
            pid=$(echo $line | awk '{print $2}')
            echo "✅ Node running (PID: $pid)"
        done
        
        if ! ps aux | grep "node run.*primary" | grep -v grep > /dev/null; then
            echo "❌ No nodes running"
        fi
        
        echo ""
        echo "📂 Log files:"
        ls -lh benchmark/logs/node*_p2p.log 2>/dev/null || echo "   (no logs yet)"
        ;;
    
    *)
        echo "Usage: $0 {start|stop|logs [1|2]|p2p|clean|status}"
        echo ""
        echo "Commands:"
        echo "  start   - Build and start 2 nodes"
        echo "  stop    - Stop all running nodes"
        echo "  logs    - Show logs (optionally specify node number)"
        echo "  p2p     - Show only P2P messages"
        echo "  clean   - Stop nodes and delete logs/stores"
        echo "  status  - Show running nodes and log files"
        exit 1
        ;;
esac
