#!/bin/bash
# Local development launch script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
export PYTHONPATH="$DIR/src:$PYTHONPATH"

if [ -f "$DIR/venv/bin/python3" ]; then
    exec "$DIR/venv/bin/python3" "$DIR/src/main.py" "$@"
else
    exec python3 "$DIR/src/main.py" "$@"
fi
