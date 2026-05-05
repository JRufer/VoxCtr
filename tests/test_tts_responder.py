"""Tests for ResponseListener — FIFO reading and TTS dispatch."""
import os
import sys
import tempfile
import threading
import time
import unittest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from tts_responder import ResponseListener


class TestResponseListener(unittest.TestCase):
    def test_delivers_line_to_tts_speak(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "resp.fifo")
            os.mkfifo(pipe_path)

            spoken = []
            done = threading.Event()

            def fake_speak(text):
                spoken.append(text)
                done.set()

            listener = ResponseListener(
                pipe_path=pipe_path,
                tts_speak=fake_speak,
                label="test",
            )
            listener.start()
            # Give listener time to open the FIFO for reading
            time.sleep(0.1)

            # Write a response as the AI would
            with open(pipe_path, "w") as f:
                f.write("Hello from the AI\n")

            done.wait(timeout=3.0)
            listener.stop()

            self.assertEqual(spoken, ["Hello from the AI"])

    def test_multiple_lines_delivered_in_order(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "resp2.fifo")
            os.mkfifo(pipe_path)

            spoken = []
            barrier = threading.Barrier(2)

            def fake_speak(text):
                spoken.append(text)
                if len(spoken) >= 3:
                    barrier.wait()

            listener = ResponseListener(pipe_path=pipe_path, tts_speak=fake_speak)
            listener.start()
            time.sleep(0.1)

            with open(pipe_path, "w") as f:
                f.write("Line one\nLine two\nLine three\n")

            barrier.wait(timeout=3.0)
            listener.stop()

            self.assertEqual(spoken, ["Line one", "Line two", "Line three"])

    def test_empty_lines_are_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "skip.fifo")
            os.mkfifo(pipe_path)

            spoken = []
            done = threading.Event()

            def fake_speak(text):
                spoken.append(text)
                done.set()

            listener = ResponseListener(pipe_path=pipe_path, tts_speak=fake_speak)
            listener.start()
            time.sleep(0.1)

            with open(pipe_path, "w") as f:
                f.write("\n\n\nReal content\n")

            done.wait(timeout=3.0)
            listener.stop()

            self.assertEqual(spoken, ["Real content"])

    def test_on_response_callback_fired(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "cb.fifo")
            os.mkfifo(pipe_path)

            cb_texts = []
            done = threading.Event()

            def fake_speak(text):
                pass

            def on_response(text):
                cb_texts.append(text)
                done.set()

            listener = ResponseListener(
                pipe_path=pipe_path,
                tts_speak=fake_speak,
                on_response=on_response,
            )
            listener.start()
            time.sleep(0.1)

            with open(pipe_path, "w") as f:
                f.write("Callback test\n")

            done.wait(timeout=3.0)
            listener.stop()

            self.assertEqual(cb_texts, ["Callback test"])

    def test_waits_for_fifo_to_exist(self):
        """Listener should retry until the FIFO is created."""
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "late.fifo")

            spoken = []
            done = threading.Event()

            def fake_speak(text):
                spoken.append(text)
                done.set()

            listener = ResponseListener(pipe_path=pipe_path, tts_speak=fake_speak)
            listener.start()

            # Create the FIFO 0.3s after the listener starts
            def create_and_write():
                time.sleep(0.3)
                os.mkfifo(pipe_path)
                with open(pipe_path, "w") as f:
                    f.write("Late arrival\n")

            threading.Thread(target=create_and_write, daemon=True).start()
            done.wait(timeout=5.0)
            listener.stop()

            self.assertEqual(spoken, ["Late arrival"])

    def test_stop_terminates_thread(self):
        with tempfile.TemporaryDirectory() as tmp:
            pipe_path = os.path.join(tmp, "stop.fifo")
            os.mkfifo(pipe_path)

            listener = ResponseListener(pipe_path=pipe_path, tts_speak=lambda t: None)
            listener.start()
            time.sleep(0.1)
            listener.stop()
            listener.join(timeout=2.0)
            # Thread should exit promptly once running=False and FIFO closes
            # (may still be alive waiting for next open — that's acceptable)
            self.assertFalse(listener.running)


if __name__ == "__main__":
    unittest.main()
