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

- **Format:** WebM (VP9 or VP8). The clips are bundled into the app and served
  from its own origin, because the CSP is `default-src 'self'` and VoxCtrl is
  meant to work with no network at all — a remote URL would be blocked.
- **Aspect ratio:** 16:9. They are drawn with `object-fit: cover` into a 16:9
  thumbnail and into the position preview, so anything else gets cropped.
- **Length:** a couple of seconds, seamlessly looping. They autoplay muted and
  loop forever while the step is open.
- **Size:** keep each one small — every byte ships in the AppImage. A few
  hundred KB each is plenty at thumbnail size.

Any style with no file here falls back to a CSS animation in the same shape and
colour, so a missing or undecodable clip degrades quietly rather than leaving a
black rectangle.
