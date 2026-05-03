#!/usr/bin/env bash
SCRIPT_DIR="$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")"
PYTHON="$SCRIPT_DIR/venv/bin/python"
LOG_DIR="$HOME/.local/share/whisper-wayland"
LOG="$LOG_DIR/app.log"
PID_FILE="$LOG_DIR/app.pid"

mkdir -p "$LOG_DIR"

# Prevent multiple instances
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Whisper-Wayland is already running (PID $(cat "$PID_FILE"))"
    exit 0
fi

if [ ! -f "$PYTHON" ]; then
    echo "Error: Virtual environment not found at $PYTHON"
    echo "Please ensure you are running this from the project directory or that the venv is correctly set up."
    exit 1
fi

export PYTHONUNBUFFERED=1
nohup "$PYTHON" "$SCRIPT_DIR/src/main.py" > "$LOG" 2>&1 &
echo $! > "$PID_FILE"
echo "Whisper-Wayland started (PID $!, log: $LOG)"
