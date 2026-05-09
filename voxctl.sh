#!/bin/bash
# Local development launch script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
export PYTHONPATH="$DIR/src:$PYTHONPATH"

if [ -f "$DIR/venv/bin/python3" ]; then
    PYTHON="$DIR/venv/bin/python3"
    PIP="$DIR/venv/bin/pip"
else
    PYTHON="python3"
    PIP="pip3"
fi

# Ensure all dependencies (including moonshine-voice) are installed before launch.
# This is a no-op when everything is already up-to-date; takes ~1s on repeat runs.
"$PIP" install -q -r "$DIR/requirements.txt" 2>/dev/null || true

exec "$PYTHON" "$DIR/src/main.py" "$@"
