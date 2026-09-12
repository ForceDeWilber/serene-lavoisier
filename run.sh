#!/usr/bin/env bash
set -e

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOGS_DIR="$DIR/logs"
PID_FILE="$DIR/.pids.json"
mkdir -p "$LOGS_DIR"

ACTION="${1:-restart}"
COMPONENT="${2:-all}"

stop_service() {
    local port="$1"
    local name="$2"
    local pid=$(lsof -ti :$port 2>/dev/null || true)
    if [ -n "$pid" ]; then
        echo "Stopping $name (PID $pid on port $port)..."
        kill -9 $pid 2>/dev/null || true
    fi
}

stop_all() {
    echo "Stopping Serene Lavoisier services..."
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "engine" ]; then
        stop_service 9099 "Engine Daemon"
    fi
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "backend" ]; then
        stop_service 8000 "FastAPI Backend"
    fi
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "frontend" ]; then
        stop_service 3000 "Next.js Frontend"
    fi
    rm -f "$PID_FILE"
    echo "Services stopped."
}

start_all() {
    echo "Starting Serene Lavoisier services..."

    # Synchronize root .env to frontend/.env.local
    if [ -f "$DIR/.env" ]; then
        cp -f "$DIR/.env" "$DIR/frontend/.env.local"
        echo "  Synchronized root .env -> frontend/.env.local"
    fi
    
    # 1. Engine Daemon
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "engine" ]; then
        if [ ! -f "$DIR/target/release/engine-daemon" ] && [ ! -f "$DIR/target/debug/engine-daemon" ]; then
            echo "Compiling Rust engine-daemon..."
            cargo build --release -p engine-daemon
        fi
        ENGINE_BIN="$DIR/target/release/engine-daemon"
        [ ! -f "$ENGINE_BIN" ] && ENGINE_BIN="$DIR/target/debug/engine-daemon"
        
        nohup "$ENGINE_BIN" > "$LOGS_DIR/engine.log" 2> "$LOGS_DIR/engine_error.log" &
        ENGINE_PID=$!
        echo "  Started Engine Daemon      [PID $ENGINE_PID] -> logs/engine.log"
    fi

    # 2. FastAPI Backend
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "backend" ]; then
        PYTHON_BIN="python3"
        [ -f "$DIR/backend/.venv/bin/python" ] && PYTHON_BIN="$DIR/backend/.venv/bin/python"
        nohup "$PYTHON_BIN" "$DIR/backend/run.py" > "$LOGS_DIR/backend.log" 2> "$LOGS_DIR/backend_error.log" &
        BACKEND_PID=$!
        echo "  Started FastAPI Backend    [PID $BACKEND_PID] -> logs/backend.log"
    fi

    # 3. Next.js Frontend
    if [ "$COMPONENT" = "all" ] || [ "$COMPONENT" = "frontend" ]; then
        cd "$DIR/frontend"
        nohup npm run dev > "$LOGS_DIR/frontend.log" 2> "$LOGS_DIR/frontend_error.log" &
        FRONTEND_PID=$!
        cd "$DIR"
        echo "  Started Next.js Dashboard  [PID $FRONTEND_PID] -> logs/frontend.log"
    fi

    sleep 2
    show_status
}

show_status() {
    echo ""
    echo "=== Serene Lavoisier // Service Status ==="
    check_port 9099 "Rust Engine Daemon" "TCP"
    check_port 8000 "FastAPI Backend"   "HTTP"
    check_port 3000 "Next.js Frontend"  "HTTP"
    echo ""
    echo "Endpoints:"
    echo "  Dashboard UI    : http://localhost:3000"
    echo "  Backend API     : http://localhost:8000/docs"
    echo "  Engine IPC      : 127.0.0.1:9099"
    echo "  Logs Folder     : ./logs/"
    echo "=========================================="
}

check_port() {
    local port="$1"
    local name="$2"
    local proto="$3"
    local pid=$(lsof -ti :$port 2>/dev/null || true)
    if [ -n "$pid" ]; then
        printf "  %-20s [%s %-4s] : \033[0;32mRUNNING\033[0m (PID: %s)\n" "$name" "$proto" "$port" "$pid"
    else
        printf "  %-20s [%s %-4s] : \033[0;31mOFFLINE\033[0m\n" "$name" "$proto" "$port"
    fi
}

case "$ACTION" in
    stop)
        stop_all
        ;;
    start)
        start_all
        ;;
    restart)
        stop_all
        sleep 1
        start_all
        ;;
    status)
        show_status
        ;;
    logs)
        tail -f "$LOGS_DIR/${2:-backend}.log"
        ;;
    *)
        echo "Usage: ./run.sh {start|stop|restart|status|logs}"
        exit 1
        ;;
esac
