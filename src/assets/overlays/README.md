# Overlay preview clips

Short silent recordings of each overlay style, shown in the setup wizard's
overlay step (step 04) so the user can see what they are choosing rather than
read a name.

Drop a file in here and it is picked up automatically — `OverlayPreview.svelte`
resolves this folder with `import.meta.glob` at build time, matching on the
style id. No code change is needed to add or replace one.

## Expected filenames

The name must be the overlay's **config id**, which is the value written to
`ui.overlay_style`, not its display name:

| Display name    | File                  |
| --------------- | --------------------- |
| Ocean Wave      | `blue_wave.webm`      |
| Voice Card      | `voice_card.webm`     |
| Waveform        | `waveform.webm`       |
| Pulse Ring      | `pulse.webm`          |
| Mono Bars       | `mono_bars.webm`      |
| Neon Spectrum   | `spectrum.webm`       |
| Retro Terminal  | `terminal.webm`       |
| Analog VU       | `vinyl.webm`          |

## What the clips should be

- **Format:** WebM. The clips are bundled into the app and served from its own
  origin, because the CSP is `default-src 'self'` and VoxCtrl is meant to work
  with no network at all — a remote URL would be blocked. Prefer VP9 or VP8:
  WebKitGTK decodes those with hardware help far more often than AV1, which it
  falls back to decoding on the CPU. (The clips currently in this folder are
  AV1 and worth re-encoding next time they are touched.)
- **Aspect ratio:** 16:9. They are drawn with `object-fit: cover` into a 16:9
  thumbnail and into the position preview, so anything else gets cropped.
- **Length:** a couple of seconds, seamlessly looping. They autoplay muted and
  loop forever while the step is open.
- **Size:** keep each one small — every byte ships in the AppImage. A few
  hundred KB each is plenty at thumbnail size.

Any style with no file here falls back to a CSS animation in the same shape and
colour, so a missing or undecodable clip degrades quietly rather than leaving a
black rectangle.

## Only two of these play at a time

WebKitGTK builds a whole GStreamer pipeline per `<video>`. Mounting one for
every style at once — eight thumbnails plus the position preview — pinned the
CPU hard enough to take the web process down with it: the step hung and then
went white. So `OverlayPreview` takes a `showClip` prop and the step only asks
for the real recording twice, on the selected thumbnail and on the large
position preview. Every other thumbnail renders the CSS fallback, which is also
animated and costs nothing. Keep it that way when adding styles: the cost here
is per element on screen, not per file in this folder.
