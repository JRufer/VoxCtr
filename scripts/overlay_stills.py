#!/usr/bin/env python3
"""Regenerate the overlay preview stills from the clips beside them.

The wizard's overlay step shows every style at once, but it cannot afford a
`<video>` for every one: WebKitGTK builds a GStreamer pipeline per element and
the clips are AV1, which it decodes on the CPU. Eight of those plus the
position preview hung the step and took the web process down with it.

So the grid shows a still of each style and only the selected thumbnail and the
large position preview play the real clip. The stills are a frame from the
middle of each clip — far enough in that the animation is in full swing rather
than fading up from black.

Usage:

    python3 scripts/overlay_stills.py            # regenerate all
    python3 scripts/overlay_stills.py pulse      # just pulse.webm

Needs PyAV (which brings its own dav1d for the AV1 clips) and Pillow:

    pip install av pillow

Run it after adding or replacing a clip in src/assets/overlays and commit the
`.webp` beside the `.webm`. A style whose still is missing still renders — it
falls back to the CSS animation — so a forgotten run degrades quietly.
"""

from __future__ import annotations

import sys
from io import BytesIO
from pathlib import Path

import av
from PIL import Image

CLIP_DIR = Path(__file__).resolve().parent.parent / "src" / "assets" / "overlays"

# Wide enough to stay sharp on a HiDPI display: the thumbnails are ~176 CSS
# pixels across and the position preview ~210, so this is comfortably 2x both.
# Height follows the clip's own display aspect, and CSS crops it to the 16:9
# frame exactly as it crops the video.
STILL_WIDTH = 480

# Visually lossless at this size, and a fraction of the clip it comes from.
WEBP_QUALITY = 88


def still_from(clip: Path) -> Image.Image:
    """Decode the frame nearest the middle of `clip`."""
    with av.open(str(clip)) as container:
        stream = container.streams.video[0]
        midpoint = container.duration // 2  # microseconds, av.time_base

        # Seeking lands on the keyframe at or before the target, so decode
        # forward from there until a frame reaches the middle. Clips this short
        # often have a single keyframe, which makes that the whole first half.
        container.seek(midpoint)
        chosen = None
        for frame in container.decode(stream):
            chosen = frame
            if frame.time is not None and frame.time * 1_000_000 >= midpoint:
                break
        if chosen is None:
            raise RuntimeError(f"{clip.name}: no frame decoded at its midpoint")

        # The clips carry a non-square sample aspect (8:9), so the frame has to
        # be resampled to its display size or the still comes out squeezed
        # against the video it is standing in for.
        dar = stream.display_aspect_ratio or (stream.width / stream.height)
        height = round(STILL_WIDTH / float(dar))
        image = chosen.to_image()

    return image.resize((STILL_WIDTH, height), Image.LANCZOS)


def main(argv: list[str]) -> int:
    wanted = {name.removesuffix(".webm") for name in argv[1:]}
    clips = sorted(CLIP_DIR.glob("*.webm"))
    if wanted:
        clips = [c for c in clips if c.stem in wanted]
        missing = wanted - {c.stem for c in clips}
        for name in sorted(missing):
            print(f"no such clip: {name}.webm", file=sys.stderr)
        if missing:
            return 1

    if not clips:
        print(f"no clips in {CLIP_DIR}", file=sys.stderr)
        return 1

    for clip in clips:
        image = still_from(clip)
        out = clip.with_suffix(".webp")
        buffer = BytesIO()
        image.save(buffer, "WEBP", quality=WEBP_QUALITY, method=6)
        out.write_bytes(buffer.getvalue())
        print(f"{out.name}: {image.width}x{image.height}, {len(buffer.getvalue()) / 1024:.0f} KB")

    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
