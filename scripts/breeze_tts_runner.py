#!/usr/bin/env python3
"""
Breeze-TTS-2 Neural Speech Inference Runner for VoxCtrl
Wraps official Breeze-TTS-2 inference runtime for realistic voice design speech generation.
"""
import argparse
import sys
import os
import subprocess
import tempfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
BREEZE_RUNNER_DIR = SCRIPT_DIR / "breeze_runner"
INFER_PY = BREEZE_RUNNER_DIR / "infer.py"

def main():
    parser = argparse.ArgumentParser(description="Breeze-TTS-2 Neural Speech Synthesizer")
    parser.add_argument("--model-dir", type=str, required=True, help="Directory containing Breeze-TTS-2 model weights")
    parser.add_argument("--prompt", type=str, default="Speak clearly and naturally.", help="Voice design speaker prompt")
    parser.add_argument("--text", type=str, required=True, help="Text to synthesize")
    parser.add_argument("--temperature", type=float, default=0.7, help="Sampling temperature")
    parser.add_argument("--gpu", action="store_true", help="Use CUDA GPU acceleration")

    args = parser.parse_args()

    if not INFER_PY.exists():
        sys.stderr.write(f"Breeze runner not found at {INFER_PY}\n")
        sys.exit(1)

    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp_wav:
        out_wav_path = Path(tmp_wav.name)

    try:
        cmd = [
            sys.executable,
            str(INFER_PY),
            args.model_dir,
            "--text", args.text,
            "--instruction", args.prompt,
            "--output", str(out_wav_path)
        ]

        env = os.environ.copy()
        env["PYTHONPATH"] = f"{BREEZE_RUNNER_DIR}:{env.get('PYTHONPATH', '')}"

        sys.stderr.write(f"Executing Breeze-TTS-2 neural inference: {' '.join(cmd)}\n")
        proc = subprocess.run(cmd, cwd=str(BREEZE_RUNNER_DIR), env=env, capture_output=True)

        if proc.returncode != 0:
            sys.stderr.write(f"Breeze-TTS-2 inference failed:\n{proc.stderr.decode('utf-8', errors='ignore')}\n")
            sys.exit(proc.returncode)

        if out_wav_path.exists() and out_wav_path.stat().st_size > 0:
            wav_bytes = out_wav_path.read_bytes()
            sys.stdout.buffer.write(wav_bytes)
            sys.stdout.buffer.flush()
        else:
            sys.stderr.write("Breeze-TTS-2 output WAV file was empty\n")
            sys.exit(1)

    finally:
        if out_wav_path.exists():
            try:
                out_wav_path.unlink()
            except Exception:
                pass

if __name__ == "__main__":
    main()
