#!/bin/bash
export PYTHONPATH="/app/share/whisper-wayland:/app/lib/python3.11/site-packages:$PYTHONPATH"
exec python3 /app/share/whisper-wayland/src/main.py "$@"
