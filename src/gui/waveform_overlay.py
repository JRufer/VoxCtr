# Backward-compatibility shim — the waveform overlay now lives in
# gui/overlays/waveform.py.  Any code that still imports WaveformOverlay
# from this module will continue to work.
from gui.overlays.waveform import OverlayUI as WaveformOverlay

__all__ = ["WaveformOverlay"]
