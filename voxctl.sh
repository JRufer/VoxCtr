#!/bin/bash
# Local development launch script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
export PYTHONPATH="$DIR/src:$PYTHONPATH"
export PATH="$DIR/piper:$PATH"
export LD_LIBRARY_PATH="$DIR/piper:$LD_LIBRARY_PATH"

# Expose host system site-packages so pyatspi (system-only package) is found
SYS_SITE=$(python3 -c "import site; print(site.getsitepackages()[0])" 2>/dev/null || true)
if [ -n "$SYS_SITE" ]; then
    export PYTHONPATH="$PYTHONPATH:$SYS_SITE"
fi

if [ -f "$DIR/.venv/bin/python3" ]; then
    exec "$DIR/.venv/bin/python3" "$DIR/src/main.py" "$@"
else
    exec python3 "$DIR/src/main.py" "$@"
fi
