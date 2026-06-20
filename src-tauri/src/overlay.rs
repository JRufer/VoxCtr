use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::BufRead;
use std::sync::{Arc, Mutex};
use i_slint_backend_winit::WinitWindowAccessor;
use slint::ComponentHandle;

slint::slint! {
    export struct BubbleData {
        y: float,
        o: float,
    }

    export struct StarData {
        x: float,
        y: float,
        r: float,
        o: float,
    }

    export struct HoloNode {
        x: float,
        y: float,
        o: float,
    }

    export component OverlayWindow inherits Window {
        // Window properties
        always-on-top: true;
        no-frame: true;
        background: transparent;
        width: 560px;
        height: 190px;

        // Status properties updated by Rust
        in property <bool> is-recording: false;
        in property <bool> is-processing: false;
        in property <bool> is-speaking: false;
        in property <bool> is-audio-ready: true;
        in property <string> target-label: "Focused Window";
        in property <string> overlay-style: "waveform";

        // Animation properties driven by the Rust spring/timer loop
        in property <float> reveal-main: 0.0;   // 0..1 load/unload progress for the visualizer
        in property <float> reveal-pill: 0.0;   // 0..1 load/unload progress for the speaking pill
        in property <float> level: 0.0;         // smoothed audio level 0..1
        in property <float> blink: 1.0;         // 0..1 pulsing value
        in property <float> sheen-x: -70.0;     // holo sheen x position (voice card)
        in property <float> buoy-y: 40.0;       // floating buoy y position (ocean)

        // Pre-built vector path data (computed in Rust each tick)
        in property <string> osc-path: "";
        in property <string> ocean-path1: "";
        in property <string> ocean-path2: "";
        in property <string> ocean-path3: "";
        in property <string> sweep-path: "";
        in property <string> sweep-trail-path: "";

        // Radar pulse rings + blips
        in property <float> ring1-s: 24.0;
        in property <float> ring1-o: 0.0;
        in property <float> ring2-s: 24.0;
        in property <float> ring2-o: 0.0;
        in property <float> blip1-o: 0.0;
        in property <float> blip2-o: 0.0;

        // LED matrix column levels (voice card) and speaking pill mini-equalizer
        in property <[float]> led-cols: [];
        in property <[float]> pill-bars: [];
        in property <[BubbleData]> bubbles: [];

        // Mono bars (mono_bars), spectrum bars (spectrum), ASCII meter
        // (terminal) and needle path (vinyl)
        in property <[float]> mono-bars: [];
        in property <[float]> spectrum-bars: [];
        in property <string> ascii-meter: "";
        in property <string> vu-needle-path: "";

        // Aurora ribbon (aurora)
        in property <string> aurora-path: "";

        // Holo ring (holo): pre-rotated orbital ring traces (computed in
        // Rust, since Path/Rectangle have no rotation-angle) + per-node
        // position/flash opacity
        in property <string> holo-ring1-path: "";
        in property <string> holo-ring2-path: "";
        in property <string> holo-ring3-path: "";
        in property <[HoloNode]> holo-nodes: [];

        // Frequency petals (petals): pre-rotated spoke paths, grouped into
        // three colour tiers (computed in Rust)
        in property <string> petal-path-a: "";
        in property <string> petal-path-b: "";
        in property <string> petal-path-c: "";

        // Minimal dot strip (dotstrip): per-dot size/brightness
        in property <[float]> dot-bars: [];

        // Starfield spectrum (starfield): fixed star field + connected
        // level path threaded through the brighter "active" stars
        in property <[StarData]> stars: [];
        in property <string> starfield-path: "";

        // ─────────────────────────────────────────────────────────────
        // 1. WAVEFORM — "OSC-01" green phosphor oscilloscope.
        //    Loads like a CRT powering on (expands from a scanline),
        //    unloads by collapsing back into the line.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "waveform" && reveal-main > 0.004) : Rectangle {
            x: 10px;
            y: 14px + (124px * (1.0 - Math.min(1.0, reveal-main))) / 2;
            width: 540px;
            height: 124px * reveal-main;
            clip: true;
            background: @linear-gradient(180deg, #07140c 0%, #030a07 100%);
            border-width: 1px;
            border-color: rgba(74, 222, 128, 0.25);
            border-radius: 10px;
            opacity: reveal-main > 0.33 ? 1.0 : reveal-main * 3.0;
            drop-shadow-blur: 18px;
            drop-shadow-color: rgba(34, 197, 94, 0.18);

            // Fixed-size inner content, vertically anchored so the
            // collapse reads as a CRT switching off.
            Rectangle {
                x: 0;
                y: (parent.height - 124px) / 2;
                width: 540px;
                height: 124px;

                // Header row
                HorizontalLayout {
                    x: 20px;
                    y: 10px;
                    width: 500px;
                    height: 18px;
                    spacing: 8px;

                    Rectangle {
                        width: 7px;
                        height: 7px;
                        border-radius: 3.5px;
                        y: (parent.height - self.height) / 2;
                        background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        drop-shadow-blur: 6px;
                        drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        opacity: 0.4 + 0.6 * blink;
                    }

                    Text {
                        text: "WAVEFORM // OSC-01";
                        color: #bbf7d0;
                        font-size: 10px;
                        font-weight: 800;
                        font-family: "Outfit";
                        letter-spacing: 1.2px;
                        vertical-alignment: center;
                    }

                    Text {
                        text: is-processing ? "· TRANSCRIBING" : (!is-audio-ready ? "· CALIBRATING" : "· LIVE TRACE");
                        color: rgba(187, 247, 208, 0.35);
                        font-size: 9px;
                        font-weight: 500;
                        font-family: "Outfit";
                        letter-spacing: 1px;
                        vertical-alignment: center;
                    }

                    Rectangle { horizontal-stretch: 1; }

                    // Target readout chip
                    Rectangle {
                        horizontal-stretch: 0;
                        background: rgba(74, 222, 128, 0.06);
                        border-width: 1px;
                        border-color: rgba(74, 222, 128, 0.3);
                        border-radius: 4px;

                        HorizontalLayout {
                            padding-left: 8px;
                            padding-right: 8px;
                            padding-top: 2px;
                            padding-bottom: 2px;
                            spacing: 5px;
                            alignment: center;

                            Text {
                                text: "TGT ▸";
                                color: #4ade80;
                                font-size: 8.5px;
                                font-weight: 800;
                                font-family: "Outfit";
                                letter-spacing: 1px;
                                vertical-alignment: center;
                            }
                            Text {
                                text: target-label;
                                color: #d1fae5;
                                font-size: 8.5px;
                                font-weight: 700;
                                font-family: "Outfit";
                                letter-spacing: 0.6px;
                                vertical-alignment: center;
                                overflow: elide;
                                max-width: 150px;
                            }
                        }
                    }
                }

                // Scope stage
                Rectangle {
                    x: 20px;
                    y: 36px;
                    width: 500px;
                    height: 78px;

                    // Horizontal graticule lines
                    Rectangle { x: 0; y: 19px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.07); }
                    Rectangle { x: 0; y: 39px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.14); }
                    Rectangle { x: 0; y: 59px; width: 500px; height: 1px; background: rgba(74, 222, 128, 0.07); }

                    // Vertical graticule ticks
                    for t in [0, 1, 2, 3, 4, 5] : Rectangle {
                        x: t * 100px;
                        y: 0;
                        width: 1px;
                        height: 78px;
                        background: rgba(74, 222, 128, 0.05);
                    }

                    // Phosphor glow pass
                    Path {
                        x: 0; y: 0;
                        width: 500px;
                        height: 78px;
                        viewbox-width: 500;
                        viewbox-height: 78;
                        commands: osc-path;
                        fill: transparent;
                        stroke: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #4ade80);
                        stroke-width: 6px;
                        opacity: 0.16;
                    }

                    // Crisp trace pass
                    Path {
                        x: 0; y: 0;
                        width: 500px;
                        height: 78px;
                        viewbox-width: 500;
                        viewbox-height: 78;
                        commands: osc-path;
                        fill: transparent;
                        stroke: is-processing ? #7dd3fc : (!is-audio-ready ? #fcd34d : #a7f3d0);
                        stroke-width: 1.8px;
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 2. PULSE RING — sonar/radar dial with rotating sweep,
        //    expanding pulse rings and a target-lock side plate.
        //    Loads by dropping/fading the dial in, plate slides out.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "pulse" && reveal-main > 0.004) : Rectangle {
            x: 0;
            y: 0;
            width: 560px;
            height: 190px;
            opacity: Math.min(1.0, reveal-main);

            // Radar dial
            Rectangle {
                x: 142px;
                y: 24px + (1.0 - reveal-main) * 30px;
                width: 124px;
                height: 124px;
                border-radius: 62px;
                clip: true;
                background: @radial-gradient(circle, #10181c 0%, #06090b 85%, #04070a 100%);
                border-width: 1.2px;
                border-color: is-processing ? rgba(56, 189, 248, 0.4) : (!is-audio-ready ? rgba(245, 158, 11, 0.4) : rgba(255, 107, 53, 0.4));
                drop-shadow-blur: 22px;
                drop-shadow-color: is-processing ? rgba(56, 189, 248, 0.2) : rgba(255, 107, 53, 0.2);

                // Static range rings
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: 92px;
                    height: 92px;
                    border-radius: 46px;
                    border-width: 1px;
                    border-color: rgba(255, 255, 255, 0.10);
                }
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: 56px;
                    height: 56px;
                    border-radius: 28px;
                    border-width: 1px;
                    border-color: rgba(255, 255, 255, 0.14);
                }

                // Crosshair axes
                Rectangle { x: 0; y: 61.5px; width: 124px; height: 1px; background: rgba(255, 255, 255, 0.07); }
                Rectangle { x: 61.5px; y: 0; width: 1px; height: 124px; background: rgba(255, 255, 255, 0.07); }

                // Sweep wedge (faded pie slice) and sweep arm
                Path {
                    x: 0; y: 0; width: 124px; height: 124px;
                    viewbox-width: 124;
                    viewbox-height: 124;
                    commands: sweep-trail-path;
                    fill: is-processing ? rgba(56, 189, 248, 0.14) : rgba(255, 107, 53, 0.14);
                    stroke: transparent;
                }
                Path {
                    x: 0; y: 0; width: 124px; height: 124px;
                    viewbox-width: 124;
                    viewbox-height: 124;
                    commands: sweep-path;
                    fill: transparent;
                    stroke: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    stroke-width: 2px;
                }

                // Expanding audio pulse rings
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: ring1-s * 1px;
                    height: ring1-s * 1px;
                    border-radius: self.width / 2;
                    border-width: 1.5px;
                    border-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    opacity: ring1-o;
                }
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: ring2-s * 1px;
                    height: ring2-s * 1px;
                    border-radius: self.width / 2;
                    border-width: 1.5px;
                    border-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    opacity: ring2-o;
                }

                // Contact blips lit up by the passing sweep
                Rectangle {
                    x: 88px; y: 42px; width: 5px; height: 5px; border-radius: 2.5px;
                    background: is-processing ? #7dd3fc : #ffb38a;
                    opacity: blip1-o;
                }
                Rectangle {
                    x: 36px; y: 86px; width: 4px; height: 4px; border-radius: 2px;
                    background: is-processing ? #7dd3fc : #ffb38a;
                    opacity: blip2-o;
                }

                // Audio-reactive core
                Rectangle {
                    x: (124px - self.width) / 2;
                    y: (124px - self.height) / 2;
                    width: (11.0 + level * 18.0) * 1px;
                    height: self.width;
                    border-radius: self.width / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                    drop-shadow-blur: 14px;
                    drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                }
            }

            // Target-lock plate
            Rectangle {
                x: 290px + (1.0 - reveal-main) * 22px;
                y: 60px;
                width: 200px;
                height: 52px;
                background: rgba(10, 12, 16, 0.94);
                border-width: 1px;
                border-color: rgba(255, 255, 255, 0.08);
                border-radius: 10px;
                opacity: Math.min(1.0, reveal-main);

                // Pulsing lock frame
                Rectangle {
                    x: 0; y: 0; width: 200px; height: 52px;
                    border-radius: 10px;
                    border-width: 1px;
                    border-color: is-processing ? rgba(56, 189, 248, 0.55) : (!is-audio-ready ? rgba(245, 158, 11, 0.55) : rgba(255, 107, 53, 0.55));
                    opacity: 0.25 + 0.75 * blink;
                }

                HorizontalLayout {
                    padding-left: 12px;
                    padding-right: 12px;
                    spacing: 10px;

                    Text {
                        text: "⌖";
                        color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #ff6b35);
                        font-size: 22px;
                        vertical-alignment: center;
                        opacity: 0.5 + 0.5 * blink;
                    }

                    VerticalLayout {
                        alignment: center;
                        spacing: 2px;

                        Text {
                            text: is-processing ? "PULSE // ANALYZING" : (!is-audio-ready ? "PULSE // ACQUIRING" : "PULSE // TARGET LOCK");
                            color: rgba(255, 255, 255, 0.35);
                            font-size: 7.5px;
                            font-weight: 800;
                            font-family: "Outfit";
                            letter-spacing: 1.4px;
                        }
                        Text {
                            text: is-processing ? "Decoding transmission…" : (!is-audio-ready ? "Connecting mic…" : target-label);
                            color: #e5e7eb;
                            font-size: 12px;
                            font-weight: 750;
                            font-family: "Outfit";
                            overflow: elide;
                            max-width: 150px;
                        }
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 3. OCEAN WAVE — glass tide pool. The water level rises with
        //    your voice; bubbles drift up and a buoy tag floats on the
        //    surface showing the target. Fills on load, drains on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "blue_wave" && reveal-main > 0.004) : Rectangle {
            x: 90px;
            y: 20px + (1.0 - reveal-main) * 16px;
            width: 380px;
            height: 128px;
            border-radius: 24px;
            clip: true;
            background: @linear-gradient(180deg, rgba(8, 18, 32, 0.96) 0%, rgba(3, 9, 18, 0.97) 100%);
            border-width: 1.2px;
            border-color: is-processing ? rgba(56, 189, 248, 0.35) : rgba(34, 211, 238, 0.25);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 24px;
            drop-shadow-color: rgba(8, 145, 178, 0.25);

            // Moonlight glow
            Rectangle {
                x: 296px; y: 10px; width: 22px; height: 22px;
                border-radius: 11px;
                background: rgba(224, 242, 254, 0.85);
                drop-shadow-blur: 16px;
                drop-shadow-color: rgba(186, 230, 253, 0.6);
                opacity: 0.55 + 0.2 * blink;
            }

            Text {
                x: 18px;
                y: 12px;
                text: "OCEAN WAVE";
                color: rgba(186, 230, 253, 0.55);
                font-size: 9.5px;
                font-weight: 800;
                font-family: "Outfit";
                letter-spacing: 2px;
            }
            Text {
                x: 18px;
                y: 25px;
                text: is-processing ? "deep current — processing" : (!is-audio-ready ? "low tide — preparing" : "high tide — listening");
                color: rgba(125, 211, 252, 0.35);
                font-size: 8px;
                font-weight: 500;
                font-family: "Outfit";
                letter-spacing: 0.8px;
            }

            // Layered water (paths rebuilt every frame in Rust)
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path1;
                fill: is-processing ? rgba(30, 64, 175, 0.40) : rgba(2, 132, 199, 0.35);
                stroke: rgba(2, 132, 199, 0.2);
                stroke-width: 1px;
            }
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path2;
                fill: is-processing ? rgba(59, 130, 246, 0.45) : rgba(6, 182, 212, 0.45);
                stroke: rgba(6, 182, 212, 0.25);
                stroke-width: 1px;
            }
            Path {
                x: 0; y: 38px; width: 380px; height: 90px;
                viewbox-width: 380; viewbox-height: 90;
                commands: ocean-path3;
                fill: is-processing ? rgba(96, 165, 250, 0.55) : rgba(34, 211, 238, 0.6);
                stroke: is-processing ? rgba(147, 197, 253, 0.6) : rgba(165, 243, 252, 0.55);
                stroke-width: 1.5px;
            }

            // Rising bubbles
            for b[i] in bubbles : Rectangle {
                x: 64px + i * 112px;
                y: b.y * 1px;
                width: i == 1 ? 6px : 4px;
                height: self.width;
                border-radius: self.width / 2;
                border-width: 1px;
                border-color: rgba(165, 243, 252, 0.7);
                background: rgba(165, 243, 252, 0.12);
                opacity: b.o;
            }

            // Floating buoy target tag (bobs on the surface)
            Rectangle {
                x: (380px - self.width) / 2;
                y: buoy-y * 1px;
                width: 168px;
                height: 24px;
                border-radius: 12px;
                background: rgba(8, 47, 73, 0.92);
                border-width: 1px;
                border-color: rgba(125, 211, 252, 0.5);

                HorizontalLayout {
                    padding-left: 10px;
                    padding-right: 12px;
                    spacing: 6px;
                    alignment: center;

                    Rectangle {
                        width: 7px;
                        height: 7px;
                        border-radius: 3.5px;
                        y: (parent.height - self.height) / 2;
                        background: is-processing ? #60a5fa : (!is-audio-ready ? #f59e0b : #22d3ee);
                        opacity: 0.5 + 0.5 * blink;
                    }
                    Text {
                        text: is-processing ? "Sounding the depths…" : (!is-audio-ready ? "Casting off…" : target-label);
                        color: #e0f2fe;
                        font-size: 10px;
                        font-weight: 700;
                        font-family: "Outfit";
                        vertical-alignment: center;
                        overflow: elide;
                        max-width: 130px;
                    }
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 4. VOICE CARD — a literal membership card: gold chip, holo
        //    sheen, VU-meter LED dot matrix and an embossed target
        //    field. Deals in with a card-flip (horizontal unfold).
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "voice_card" && reveal-main > 0.004) : Rectangle {
            width: 340px * Math.max(0.001, reveal-main);
            x: (560px - self.width) / 2;
            y: 16px;
            height: 152px;
            clip: true;
            border-radius: 16px;
            background: @linear-gradient(135deg, #181b24 0%, #0b0d12 55%, #141925 100%);
            border-width: 1px;
            border-color: is-processing ? rgba(56, 189, 248, 0.4) : rgba(255, 255, 255, 0.12);
            opacity: reveal-main > 0.5 ? 1.0 : reveal-main * 2.0;
            drop-shadow-blur: 26px;
            drop-shadow-color: rgba(0, 0, 0, 0.55);

            // Fixed-width inner face, centre-anchored so the unfold
            // reads like the card flipping open.
            Rectangle {
                x: (parent.width - 340px) / 2;
                y: 0;
                width: 340px;
                height: 152px;

                // Holographic sheen sweeping across the card
                Rectangle {
                    x: sheen-x * 1px;
                    y: -20px;
                    width: 64px;
                    height: 192px;
                    background: @linear-gradient(90deg, rgba(255, 255, 255, 0) 0%, rgba(255, 255, 255, 0.06) 50%, rgba(255, 255, 255, 0) 100%);
                }

                // Gold chip
                Rectangle {
                    x: 20px; y: 18px; width: 38px; height: 28px;
                    border-radius: 6px;
                    background: @linear-gradient(160deg, #f6d27a 0%, #d9a946 55%, #b9842b 100%);

                    Rectangle { x: 0; y: 9px; width: 38px; height: 1.5px; background: rgba(70, 45, 0, 0.45); }
                    Rectangle { x: 0; y: 18px; width: 38px; height: 1.5px; background: rgba(70, 45, 0, 0.45); }
                    Rectangle { x: 18px; y: 0; width: 1.5px; height: 28px; background: rgba(70, 45, 0, 0.45); }
                }

                Text {
                    x: 68px; y: 16px;
                    text: "VOXCTRL";
                    color: #f3f4f6;
                    font-size: 14px;
                    font-weight: 850;
                    font-family: "Outfit";
                    letter-spacing: 1px;
                }
                Text {
                    x: 68px; y: 34px;
                    text: "VOICE CARD";
                    color: rgba(255, 255, 255, 0.38);
                    font-size: 7.5px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 3px;
                }

                // Status stamp (top right)
                Rectangle {
                    x: 252px; y: 18px; width: 68px; height: 20px;
                    border-radius: 5px;
                    border-width: 1px;
                    border-color: is-processing ? rgba(56, 189, 248, 0.5) : (!is-audio-ready ? rgba(245, 158, 11, 0.5) : rgba(244, 63, 94, 0.5));
                    background: is-processing ? rgba(56, 189, 248, 0.08) : (!is-audio-ready ? rgba(245, 158, 11, 0.08) : rgba(244, 63, 94, 0.08));

                    HorizontalLayout {
                        alignment: center;
                        spacing: 5px;

                        Rectangle {
                            width: 6px; height: 6px; border-radius: 3px;
                            y: (parent.height - self.height) / 2;
                            background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #f43f5e);
                            opacity: 0.35 + 0.65 * blink;
                        }
                        Text {
                            text: is-processing ? "PROC" : (!is-audio-ready ? "INIT" : "REC");
                            color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #f43f5e);
                            font-size: 8.5px;
                            font-weight: 850;
                            font-family: "Outfit";
                            letter-spacing: 1.5px;
                            vertical-alignment: center;
                        }
                    }
                }

                // VU-meter LED dot matrix (green→amber→red, bottom-up)
                HorizontalLayout {
                    x: 22px;
                    y: 56px;
                    width: 296px;
                    height: 51px;
                    spacing: 4px;

                    for col-level[ci] in led-cols : VerticalLayout {
                        spacing: 3px;

                        for r in [0, 1, 2, 3, 4, 5] : Rectangle {
                            width: 11px;
                            height: 6px;
                            border-radius: 2px;
                            background: is-processing ? #22d3ee : (r == 0 ? #f43f5e : (r <= 2 ? #fbbf24 : #34d399));
                            opacity: (6.0 - r) <= col-level * 6.0 ? 0.95 : 0.10;
                        }
                    }
                }

                // Embossed target field (bottom left)
                Text {
                    x: 22px; y: 118px;
                    text: "TARGET";
                    color: rgba(255, 255, 255, 0.30);
                    font-size: 7px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 2.5px;
                }
                Text {
                    x: 22px; y: 128px;
                    text: is-processing ? "Reading the card…" : target-label;
                    color: #e5e7eb;
                    font-size: 11.5px;
                    font-weight: 750;
                    font-family: "Outfit";
                    letter-spacing: 0.5px;
                    overflow: elide;
                    max-width: 200px;
                }

                // Card number flourish (bottom right)
                Text {
                    x: 236px; y: 128px;
                    text: "•••• VOX";
                    color: rgba(255, 255, 255, 0.22);
                    font-size: 11px;
                    font-weight: 700;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 5. MONO BARS — hyper-minimal black & white 5-bar level meter.
        //    Pure greyscale: no color, no gradients, no glow. Fades in
        //    and the bars grow from the baseline on load, shrink back
        //    on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "mono_bars" && reveal-main > 0.004) : Rectangle {
            x: (560px - 190px) / 2;
            y: 39px;
            width: 190px;
            height: 112px;
            clip: true;
            background: #000000;
            border-width: 1px;
            border-color: rgba(255, 255, 255, 0.16);
            border-radius: 6px;
            opacity: Math.min(1.0, reveal-main);

            // Status row
            HorizontalLayout {
                x: 18px;
                y: 14px;
                width: 154px;
                height: 12px;
                spacing: 6px;

                Rectangle {
                    width: 6px;
                    height: 6px;
                    y: (parent.height - self.height) / 2;
                    border-radius: 3px;
                    border-width: 1px;
                    border-color: rgba(245, 245, 245, 0.7);
                    background: (!is-processing && is-audio-ready) ? #f5f5f5 : transparent;
                    opacity: is-processing ? (0.3 + 0.7 * blink) : (!is-audio-ready ? 0.35 : (0.5 + 0.5 * blink));
                }

                Text {
                    text: is-processing ? "PROCESSING" : (!is-audio-ready ? "STANDBY" : "LISTENING");
                    color: rgba(255, 255, 255, 0.55);
                    font-size: 8px;
                    font-weight: 700;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                    vertical-alignment: center;
                }
            }

            // 5-bar minimal wave
            for h[i] in mono-bars : Rectangle {
                x: 37px + i * 26px;
                y: 84px - h * Math.min(1.0, reveal-main) * 1px;
                width: 12px;
                height: h * Math.min(1.0, reveal-main) * 1px;
                border-radius: 6px;
                background: #f5f5f5;
                opacity: is-processing ? 0.55 : (!is-audio-ready ? 0.3 : 1.0);
            }

            // Baseline
            Rectangle { x: 18px; y: 84px; width: 154px; height: 1px; background: rgba(255, 255, 255, 0.12); }

            // Target label
            Text {
                x: 18px;
                y: 92px;
                width: 154px;
                height: 14px;
                text: target-label;
                color: rgba(255, 255, 255, 0.4);
                font-size: 8.5px;
                font-weight: 600;
                font-family: "Outfit";
                letter-spacing: 0.5px;
                overflow: elide;
                horizontal-alignment: center;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 6. NEON SPECTRUM — a 16-band equalizer in a magenta-to-cyan
        //    gradient. Powers up from the floor: the panel's height
        //    grows while the content stays anchored to the bottom edge,
        //    so the bars appear to rise into view. Reverses on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "spectrum" && reveal-main > 0.004) : Rectangle {
            x: 60px;
            y: 150px - 132px * Math.min(1.0, reveal-main);
            width: 440px;
            height: 132px * Math.min(1.0, reveal-main);
            clip: true;
            border-radius: 16px;
            background: @linear-gradient(160deg, #1b0c2e 0%, #0a0614 100%);
            border-width: 1px;
            border-color: rgba(232, 121, 249, 0.25);
            opacity: reveal-main > 0.2 ? 1.0 : reveal-main * 5.0;
            drop-shadow-blur: 26px;
            drop-shadow-color: rgba(192, 132, 252, 0.22);

            // Fixed-size inner content, bottom-anchored so the bars read
            // as rising up out of the floor as the panel grows.
            Rectangle {
                x: 0;
                y: parent.height - 132px;
                width: 440px;
                height: 132px;

                // Header
                HorizontalLayout {
                    x: 20px; y: 12px; width: 400px; height: 16px; spacing: 8px;

                    Rectangle {
                        width: 7px; height: 7px; border-radius: 3.5px;
                        y: (parent.height - self.height) / 2;
                        background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #e879f9);
                        drop-shadow-blur: 6px;
                        drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #e879f9);
                        opacity: 0.4 + 0.6 * blink;
                    }
                    Text {
                        text: "SPECTRUM // EQ-16";
                        color: #f5d0fe;
                        font-size: 10px;
                        font-weight: 800;
                        font-family: "Outfit";
                        letter-spacing: 1.5px;
                        vertical-alignment: center;
                    }
                    Text {
                        text: is-processing ? "· ANALYZING" : (!is-audio-ready ? "· WARMING UP" : "· LIVE");
                        color: rgba(245, 208, 254, 0.35);
                        font-size: 9px;
                        font-weight: 500;
                        font-family: "Outfit";
                        letter-spacing: 1px;
                        vertical-alignment: center;
                    }
                    Rectangle { horizontal-stretch: 1; }
                    Rectangle {
                        horizontal-stretch: 0;
                        background: rgba(232, 121, 249, 0.08);
                        border-width: 1px;
                        border-color: rgba(232, 121, 249, 0.3);
                        border-radius: 4px;

                        HorizontalLayout {
                            padding-left: 8px; padding-right: 8px; padding-top: 2px; padding-bottom: 2px;
                            spacing: 5px; alignment: center;

                            Text {
                                text: "OUT ▸";
                                color: #e879f9;
                                font-size: 8.5px;
                                font-weight: 800;
                                font-family: "Outfit";
                                letter-spacing: 1px;
                                vertical-alignment: center;
                            }
                            Text {
                                text: target-label;
                                color: #fae8ff;
                                font-size: 8.5px;
                                font-weight: 700;
                                font-family: "Outfit";
                                letter-spacing: 0.6px;
                                vertical-alignment: center;
                                overflow: elide;
                                max-width: 150px;
                            }
                        }
                    }
                }

                // Equalizer bars
                for h[i] in spectrum-bars : Rectangle {
                    x: 20px + i * 25.5px;
                    y: 116px - h * 1px;
                    width: 19px;
                    height: h * 1px;
                    border-radius: 3px;
                    background: @linear-gradient(180deg, #f0abfc 0%, #c084fc 45%, #38bdf8 100%);
                    opacity: 0.45 + 0.55 * (h / 96.0);
                }

                // Floor line
                Rectangle { x: 20px; y: 116px; width: 400px; height: 1px; background: rgba(232, 121, 249, 0.15); }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 7. RETRO TERMINAL — a DOS-blue console window with monospace
        //    readouts and a block-character level meter. Drops down
        //    from the top edge on load, retracts back up on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "terminal" && reveal-main > 0.004) : Rectangle {
            x: 90px;
            y: 16px;
            width: 380px;
            height: 130px * Math.min(1.0, reveal-main);
            clip: true;
            background: #0d1b4c;
            border-width: 1.5px;
            border-color: rgba(125, 211, 252, 0.35);
            border-radius: 8px;
            opacity: reveal-main > 0.2 ? 1.0 : reveal-main * 5.0;
            drop-shadow-blur: 20px;
            drop-shadow-color: rgba(8, 15, 48, 0.6);

            // Title bar
            Rectangle {
                x: 0; y: 0; width: 380px; height: 24px;
                background: rgba(255, 255, 255, 0.06);

                HorizontalLayout {
                    padding-left: 12px;
                    spacing: 6px;

                    Rectangle { width: 8px; height: 8px; border-radius: 4px; y: (parent.height - self.height) / 2; background: rgba(255, 255, 255, 0.22); }
                    Rectangle { width: 8px; height: 8px; border-radius: 4px; y: (parent.height - self.height) / 2; background: rgba(255, 255, 255, 0.22); }
                    Rectangle { width: 8px; height: 8px; border-radius: 4px; y: (parent.height - self.height) / 2; background: rgba(255, 255, 255, 0.22); }

                    Text {
                        text: "VOXCTRL — /dev/mic0";
                        color: rgba(224, 242, 254, 0.55);
                        font-size: 9px;
                        font-weight: 700;
                        font-family: "Outfit";
                        letter-spacing: 1px;
                        vertical-alignment: center;
                    }
                }
            }

            // Body
            VerticalLayout {
                x: 16px; y: 34px; width: 348px; height: 88px;
                spacing: 8px;

                Text {
                    text: "$ voxctrl listen --target \"" + target-label + "\"";
                    color: rgba(186, 230, 253, 0.7);
                    font-size: 10px;
                    font-weight: 600;
                    font-family: "monospace";
                    overflow: elide;
                }

                Text {
                    text: (is-processing ? "[PROC] " : (!is-audio-ready ? "[INIT] " : "[REC ] ")) + ascii-meter;
                    color: is-processing ? #7dd3fc : (!is-audio-ready ? #fcd34d : #ffffff);
                    font-size: 11px;
                    font-weight: 700;
                    font-family: "monospace";
                    letter-spacing: 1px;
                }

                Text {
                    text: (is-processing ? "transcribing audio stream" : (!is-audio-ready ? "connecting input device" : "streaming to output")) + (blink > 0.5 ? "_" : " ");
                    color: rgba(186, 230, 253, 0.45);
                    font-size: 9.5px;
                    font-weight: 500;
                    font-family: "monospace";
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 8. ANALOG VU — a warm vintage VU meter with a spring-loaded
        //    needle that kicks and settles with your voice. Fades in
        //    and rises slightly into place on load, settles back down
        //    on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "vinyl" && reveal-main > 0.004) : Rectangle {
            x: 120px;
            y: 24px + (1.0 - Math.min(1.0, reveal-main)) * 16px;
            width: 320px;
            height: 132px;
            clip: true;
            border-radius: 14px;
            background: @linear-gradient(180deg, #f7ecd9 0%, #e7d6b8 100%);
            border-width: 1.5px;
            border-color: rgba(120, 89, 53, 0.35);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 22px;
            drop-shadow-color: rgba(0, 0, 0, 0.35);

            // Header
            HorizontalLayout {
                x: 16px; y: 12px; width: 288px; height: 14px; spacing: 8px;

                Text {
                    text: "VU";
                    color: #5b4530;
                    font-size: 12px;
                    font-weight: 900;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                    vertical-alignment: center;
                }
                Text {
                    text: is-processing ? "ANALOG // PROCESSING" : (!is-audio-ready ? "ANALOG // WARMING UP" : "ANALOG // INPUT LEVEL");
                    color: rgba(91, 69, 48, 0.55);
                    font-size: 8px;
                    font-weight: 700;
                    font-family: "Outfit";
                    letter-spacing: 1.5px;
                    vertical-alignment: center;
                }
                Rectangle { horizontal-stretch: 1; }
                Rectangle {
                    horizontal-stretch: 0;
                    width: 8px; height: 8px; border-radius: 4px;
                    y: (parent.height - self.height) / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #dc2626);
                    drop-shadow-blur: 6px;
                    drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #dc2626);
                    opacity: 0.4 + 0.6 * blink;
                }
            }

            // Meter face
            Rectangle {
                x: 16px; y: 30px; width: 288px; height: 72px;
                border-radius: 8px;
                background: rgba(255, 252, 244, 0.55);
                border-width: 1px;
                border-color: rgba(120, 89, 53, 0.25);

                // Scale ticks (the rightmost, in the red, is the +3dB mark)
                for t in [0, 1, 2, 3, 4, 5, 6] : Rectangle {
                    x: 18px + t * 42px;
                    y: 14px;
                    width: 1.5px;
                    height: t == 6 ? 30px : 22px;
                    background: t == 6 ? #dc2626 : rgba(91, 69, 48, 0.4);
                }
                Text { x: 12px; y: 40px; text: "-20"; color: rgba(91, 69, 48, 0.5); font-size: 7px; font-family: "Outfit"; }
                Text { x: 122px; y: 40px; text: "0"; color: rgba(91, 69, 48, 0.5); font-size: 7px; font-family: "Outfit"; }
                Text { x: 252px; y: 34px; text: "+3"; color: #dc2626; font-size: 7px; font-weight: 800; font-family: "Outfit"; }

                // Needle (path rebuilt every frame in Rust, spring-driven)
                Path {
                    x: 0; y: 0; width: 288px; height: 72px;
                    viewbox-width: 288; viewbox-height: 72;
                    commands: vu-needle-path;
                    stroke: #292524;
                    stroke-width: 2px;
                    fill: transparent;
                }

                // Pivot cap
                Rectangle {
                    x: 138px; y: 64px; width: 12px; height: 12px; border-radius: 6px;
                    background: #292524;
                }
            }

            // Target label
            Text {
                x: 16px; y: 110px;
                width: 288px;
                text: target-label;
                color: rgba(91, 69, 48, 0.65);
                font-size: 9.5px;
                font-weight: 700;
                font-family: "Outfit";
                overflow: elide;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 9. AURORA RIBBON — a flowing gradient ribbon that bends with
        //    your voice, glowing cyan → violet → pink. Fades in with a
        //    gentle rise, fades out the same way on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "aurora" && reveal-main > 0.004) : Rectangle {
            x: 28px;
            y: 32px + (1.0 - Math.min(1.0, reveal-main)) * 14px;
            width: 504px;
            height: 126px;
            clip: true;
            border-radius: 16px;
            background: @linear-gradient(165deg, rgba(8, 10, 18, 0.92) 0%, rgba(5, 6, 12, 0.96) 100%);
            border-width: 1px;
            border-color: rgba(168, 139, 250, 0.22);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 30px;
            drop-shadow-color: rgba(124, 58, 237, 0.18);

            HorizontalLayout {
                x: 22px; y: 16px; width: 460px; height: 14px; spacing: 8px;

                Rectangle {
                    width: 6px; height: 6px; border-radius: 3px;
                    y: (parent.height - self.height) / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #a78bfa);
                    drop-shadow-blur: 7px;
                    drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #a78bfa);
                    opacity: 0.4 + 0.6 * blink;
                }
                Text {
                    text: "AURORA";
                    color: #e9d5ff;
                    font-size: 10px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                    vertical-alignment: center;
                }
                Text {
                    text: is-processing ? "· DRIFTING" : (!is-audio-ready ? "· WAKING" : "· LIVE");
                    color: rgba(233, 213, 255, 0.4);
                    font-size: 9px;
                    font-weight: 500;
                    font-family: "Outfit";
                    letter-spacing: 1px;
                    vertical-alignment: center;
                }
                Rectangle { horizontal-stretch: 1; }
                Text {
                    horizontal-stretch: 0;
                    text: target-label;
                    color: rgba(233, 213, 255, 0.55);
                    font-size: 9px;
                    font-weight: 700;
                    font-family: "Outfit";
                    vertical-alignment: center;
                    overflow: elide;
                    max-width: 150px;
                }
            }

            // Glow pass beneath the ribbon
            Path {
                x: 0; y: 38px; width: 504px; height: 78px;
                viewbox-width: 504; viewbox-height: 78;
                commands: aurora-path;
                fill: transparent;
                stroke: #a78bfa;
                stroke-width: 12px;
                opacity: 0.18;
            }
            // Crisp ribbon trace, tri-tone (cyan / violet / pink) drawn as
            // three colour passes so the line reads as a gradient.
            Path {
                x: 0; y: 38px; width: 168px; height: 78px;
                viewbox-width: 168; viewbox-height: 78;
                commands: aurora-path;
                clip: true;
                fill: transparent;
                stroke: #67e8f9;
                stroke-width: 3px;
            }
            Path {
                x: 168px; y: 38px; width: 168px; height: 78px;
                viewbox-width: 504; viewbox-height: 78;
                viewbox-x: 168;
                commands: aurora-path;
                clip: true;
                fill: transparent;
                stroke: #c4b5fd;
                stroke-width: 3px;
            }
            Path {
                x: 336px; y: 38px; width: 168px; height: 78px;
                viewbox-width: 504; viewbox-height: 78;
                viewbox-x: 336;
                commands: aurora-path;
                clip: true;
                fill: transparent;
                stroke: #f9a8d4;
                stroke-width: 3px;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 10. HOLO RING — rotating holographic orbital rings (like an
        //     atom) with particle nodes that flash as the sweep passes.
        //     Scales up and fades in on load, shrinks back on unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "holo" && reveal-main > 0.004) : Rectangle {
            width: 250px * Math.max(0.001, Math.min(1.0, 0.4 + reveal-main * 0.6));
            height: 190px * Math.max(0.001, Math.min(1.0, 0.4 + reveal-main * 0.6));
            x: (560px - self.width) / 2;
            y: (190px - self.height) / 2;
            opacity: Math.min(1.0, reveal-main);

            Text {
                x: 24px; y: 16px;
                text: "HOLO RING";
                color: #f5d0fe;
                font-size: 10px;
                font-weight: 800;
                font-family: "Outfit";
                letter-spacing: 2px;
            }
            Rectangle {
                x: 14px; y: 17px; width: 6px; height: 6px; border-radius: 3px;
                background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #e879f9);
                opacity: 0.4 + 0.6 * blink;
            }

            Rectangle {
                x: (parent.width - 180px) / 2;
                y: (parent.height - 180px) / 2;
                width: 180px;
                height: 180px;

                // Three orbital rings, evenly spaced in rotation. The
                // rotated ellipse trace is pre-computed in Rust each tick
                // (Path has no rotation-angle property in Slint).
                Path {
                    x: 0; y: 0; width: 180px; height: 180px;
                    viewbox-width: 180; viewbox-height: 180;
                    commands: holo-ring1-path;
                    fill: transparent;
                    stroke: #67e8f9;
                    stroke-width: 1.4px;
                    opacity: 0.42;
                }
                Path {
                    x: 0; y: 0; width: 180px; height: 180px;
                    viewbox-width: 180; viewbox-height: 180;
                    commands: holo-ring2-path;
                    fill: transparent;
                    stroke: #d946ef;
                    stroke-width: 1.4px;
                    opacity: 0.42;
                }
                Path {
                    x: 0; y: 0; width: 180px; height: 180px;
                    viewbox-width: 180; viewbox-height: 180;
                    commands: holo-ring3-path;
                    fill: transparent;
                    stroke: #f9a8d4;
                    stroke-width: 1.4px;
                    opacity: 0.42;
                }

                // Particle nodes around the rim, flashing as the sweep
                // passes. Positions are pre-computed in Rust (Math.cos/sin
                // need angle-typed arguments, not raw floats).
                for nd[ni] in holo-nodes : Rectangle {
                    width: 4.5px; height: 4.5px; border-radius: 2.25px;
                    x: nd.x * 1px - 2.25px;
                    y: nd.y * 1px - 2.25px;
                    background: #f0abfc;
                    opacity: nd.o;
                    drop-shadow-blur: 5px;
                    drop-shadow-color: #f0abfc;
                }

                // Audio-reactive core
                Rectangle {
                    width: (10px + level * 10px);
                    height: self.width;
                    border-radius: self.width / 2;
                    x: 90px - self.width / 2;
                    y: 90px - self.height / 2;
                    background: #ffffff;
                    opacity: 0.85 + 0.15 * blink;
                    drop-shadow-blur: 14px;
                    drop-shadow-color: is-processing ? #38bdf8 : #e879f9;
                }
            }

            Text {
                x: (parent.width - self.width) / 2;
                y: parent.height - 24px;
                width: 220px;
                text: is-processing ? "decoding…" : (!is-audio-ready ? "calibrating…" : target-label);
                color: rgba(245, 208, 254, 0.6);
                font-size: 9.5px;
                font-weight: 700;
                font-family: "Outfit";
                horizontal-alignment: center;
                overflow: elide;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 11. FREQUENCY PETALS — a radial mandala of spokes blooming
        //     outward from a centre point, colour-shifting magenta to
        //     violet with intensity. Blooms open on load, close on
        //     unload.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "petals" && reveal-main > 0.004) : Rectangle {
            width: 190px;
            height: 190px;
            x: (560px - self.width) / 2;
            y: 0;
            opacity: Math.min(1.0, reveal-main);

            Rectangle {
                x: (parent.width - 170px) / 2;
                y: 6px;
                width: 170px;
                height: 170px;

                // Each spoke is a pre-rotated capsule path computed in Rust
                // (Rectangle has no rotation-angle property in Slint),
                // grouped into three colour tiers for a gradient-like look.
                Path {
                    x: 0; y: 0; width: 170px; height: 170px;
                    viewbox-width: 170; viewbox-height: 170;
                    commands: petal-path-a;
                    fill: #fdf4ff;
                    opacity: 0.85;
                }
                Path {
                    x: 0; y: 0; width: 170px; height: 170px;
                    viewbox-width: 170; viewbox-height: 170;
                    commands: petal-path-b;
                    fill: #f0abfc;
                    opacity: 0.85;
                }
                Path {
                    x: 0; y: 0; width: 170px; height: 170px;
                    viewbox-width: 170; viewbox-height: 170;
                    commands: petal-path-c;
                    fill: #a78bfa;
                    opacity: 0.85;
                }

                // Center hub
                Rectangle {
                    width: 22px; height: 22px; border-radius: 11px;
                    x: 85px - 11px; y: 85px - 11px;
                    background: #fdf4ff;
                    opacity: 0.85 + 0.15 * blink;
                    drop-shadow-blur: 14px;
                    drop-shadow-color: #e879f9;
                }
            }

            Text {
                x: (parent.width - self.width) / 2;
                y: parent.height - 20px;
                width: 170px;
                text: is-processing ? "blooming…" : (!is-audio-ready ? "budding…" : target-label);
                color: rgba(253, 244, 255, 0.55);
                font-size: 9px;
                font-weight: 700;
                font-family: "Outfit";
                horizontal-alignment: center;
                overflow: elide;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 12. MINIMAL DOT STRIP — an ultra-compact pill of dots that
        //     light up and grow with your voice. The most unobtrusive
        //     style. Fades and slightly grows in on load.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "dotstrip" && reveal-main > 0.004) : Rectangle {
            width: 220px * Math.max(0.001, 0.85 + Math.min(1.0, reveal-main) * 0.15);
            height: 56px;
            x: (560px - self.width) / 2;
            y: (190px - self.height) / 2;
            border-radius: 28px;
            background: rgba(10, 10, 12, 0.88);
            border-width: 1px;
            border-color: rgba(255, 255, 255, 0.08);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 22px;
            drop-shadow-color: rgba(0, 0, 0, 0.5);

            HorizontalLayout {
                alignment: center;
                spacing: 7px;

                for s[i] in dot-bars : Rectangle {
                    width: 7px * s;
                    height: self.width;
                    y: (parent.height - self.height) / 2;
                    border-radius: self.width / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? rgba(255, 255, 255, 0.18) : #ff6b35);
                    opacity: 0.35 + 0.65 * Math.min(1.0, (s - 1.0) / 1.4);
                    drop-shadow-blur: s > 1.5 ? 6px : 0px;
                    drop-shadow-color: is-processing ? #38bdf8 : #ff6b35;
                }
            }
        }

        // ─────────────────────────────────────────────────────────────
        // 13. STARFIELD SPECTRUM — a constellation over a deep indigo
        //     field, with a connected waveform thread running through
        //     the brighter "active" stars. Fades in, drifts slightly.
        // ─────────────────────────────────────────────────────────────
        if (overlay-style == "starfield" && reveal-main > 0.004) : Rectangle {
            x: 28px;
            y: 32px + (1.0 - Math.min(1.0, reveal-main)) * 14px;
            width: 504px;
            height: 126px;
            clip: true;
            border-radius: 16px;
            background: @radial-gradient(circle, #131b35 0%, #05060c 70%);
            border-width: 1px;
            border-color: rgba(99, 102, 241, 0.25);
            opacity: Math.min(1.0, reveal-main);
            drop-shadow-blur: 28px;
            drop-shadow-color: rgba(99, 102, 241, 0.18);

            HorizontalLayout {
                x: 22px; y: 16px; width: 460px; height: 14px; spacing: 8px;

                Rectangle {
                    width: 6px; height: 6px; border-radius: 3px;
                    y: (parent.height - self.height) / 2;
                    background: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #818cf8);
                    drop-shadow-blur: 7px;
                    drop-shadow-color: is-processing ? #38bdf8 : (!is-audio-ready ? #f59e0b : #818cf8);
                    opacity: 0.4 + 0.6 * blink;
                }
                Text {
                    text: "STARFIELD";
                    color: #c7d2fe;
                    font-size: 10px;
                    font-weight: 800;
                    font-family: "Outfit";
                    letter-spacing: 2px;
                    vertical-alignment: center;
                }
                Rectangle { horizontal-stretch: 1; }
                Text {
                    horizontal-stretch: 0;
                    text: target-label;
                    color: rgba(199, 210, 254, 0.55);
                    font-size: 9px;
                    font-weight: 700;
                    font-family: "Outfit";
                    vertical-alignment: center;
                    overflow: elide;
                    max-width: 150px;
                }
            }

            // Background stars
            for st[i] in stars : Rectangle {
                width: st.r * 1px;
                height: self.width;
                border-radius: self.width / 2;
                x: st.x * 1px;
                y: st.y * 1px;
                background: #e0e7ff;
                opacity: st.o;
            }

            // Connected level thread running through the active stars
            Path {
                x: 0; y: 0; width: 504px; height: 126px;
                viewbox-width: 504; viewbox-height: 126;
                commands: starfield-path;
                fill: transparent;
                stroke: is-processing ? #7dd3fc : #818cf8;
                stroke-width: 1.5px;
                opacity: 0.7;
            }
        }

        // ─────────────────────────────────────────────────────────────
        // Speaking pill — slides up from the bottom while the system
        // responds, with a live mini-equalizer.
        // ─────────────────────────────────────────────────────────────
        if (reveal-pill > 0.004) : Rectangle {
            x: (560px - self.width) / 2;
            y: 134px + (1.0 - reveal-pill) * 20px;
            width: 280px;
            height: 46px;
            border-radius: 23px;
            background: rgba(4, 47, 36, 0.94);
            border-width: 1.2px;
            border-color: rgba(16, 185, 129, 0.55);
            opacity: Math.min(1.0, reveal-pill);
            drop-shadow-blur: 18px;
            drop-shadow-color: rgba(16, 185, 129, 0.25);

            HorizontalLayout {
                padding-left: 18px;
                padding-right: 18px;
                spacing: 10px;

                // Mini equalizer
                Rectangle {
                    width: 26px;

                    for h[i] in pill-bars : Rectangle {
                        x: i * 5.5px;
                        y: (parent.height - self.height) / 2;
                        width: 3px;
                        height: h * 1px;
                        border-radius: 1.5px;
                        background: #34d399;
                    }
                }

                VerticalLayout {
                    alignment: center;
                    spacing: 1px;

                    Text {
                        text: "SYSTEM RESPONDING";
                        color: #a7f3d0;
                        font-size: 10px;
                        font-weight: 850;
                        font-family: "Outfit";
                        letter-spacing: 1.5px;
                    }
                    Text {
                        text: "▸ " + target-label;
                        color: rgba(167, 243, 208, 0.5);
                        font-size: 8.5px;
                        font-weight: 600;
                        font-family: "Outfit";
                        overflow: elide;
                        max-width: 180px;
                    }
                }
            }
        }
    }
}

struct AppState {
    recording: bool,
    processing: bool,
    speaking: bool,
    audio_ready: bool,
    audio_level: f32,
    active_target_label: String,
    x: i32,
    y: i32,
    overlay_style: String,
}

/// Critically-damped-ish spring used for load/unload (reveal) animations.
/// Slightly underdamped so overlays land with a subtle overshoot.
fn spring(x: &mut f32, v: &mut f32, target: f32, dt: f32) {
    let omega = 16.0_f32;
    let zeta = 0.78_f32;
    let a = omega * omega * (target - *x) - 2.0 * zeta * omega * *v;
    *v += a * dt;
    *x += *v * dt;
    if (*x - target).abs() < 0.001 && v.abs() < 0.02 {
        *x = target;
        *v = 0.0;
    }
}

/// Build a filled sine-wave path (for the ocean layers). The shape is always
/// closed along the bottom edge (`y = height`) so the waterline animates while
/// the bottom of the water stays locked to the bottom of the visualizer.
fn ocean_wave_path(width: f32, height: f32, amp: f32, freq: f32, phase: f32, y_off: f32) -> String {
    let mut d = String::with_capacity(1024);
    let _ = write!(d, "M 0 {height} L 0 {y_off:.1}");
    let mut x = 0.0_f32;
    while x <= width {
        let y = y_off + (x * freq + phase).sin() * amp;
        let _ = write!(d, " L {x:.0} {y:.1}");
        x += 8.0;
    }
    let _ = write!(d, " L {width} {height} Z");
    d
}

// ── Oscilloscope (waveform style) ────────────────────────────────────
const OSC_MID: f32 = 39.0; // vertical centre of the 78px scope stage
const OSC_AMP: f32 = 35.0; // max trace deflection in px

/// Next sample pushed into the oscilloscope ring buffer. `noise` is a random
/// value in -1..1.
fn osc_sample(recording: bool, processing: bool, ready: bool, level: f32, phase: f32, noise: f32) -> f32 {
    if processing {
        0.55 * (phase * 9.0).sin() * (0.6 + 0.4 * (phase * 1.7).sin())
    } else if recording && !ready {
        0.05 * noise
    } else if recording {
        (level * 1.5).min(1.0) * (0.45 * (phase * 24.0).sin() + 0.55 * noise)
    } else {
        0.0
    }
}

/// Render the sample history as an SVG polyline. Positive samples deflect the
/// trace upwards (smaller y), like a real scope.
fn build_osc_path(hist: &VecDeque<f32>) -> String {
    let mut d = String::with_capacity(2048);
    for (i, v) in hist.iter().enumerate() {
        let x = i as f32 * 4.0;
        let y = (OSC_MID - v * OSC_AMP).clamp(2.0, 76.0);
        if i == 0 {
            let _ = write!(d, "M {x:.0} {y:.1}");
        } else {
            let _ = write!(d, " L {x:.0} {y:.1}");
        }
    }
    d
}

// ── Radar (pulse style) ──────────────────────────────────────────────
const RADAR_C: f32 = 62.0; // dial centre (124px dial)
const RADAR_R: f32 = 56.0; // sweep radius
const RADAR_WEDGE_SPAN: f32 = 0.95; // radians of trailing wedge

/// The bright sweep arm: a line from the dial centre to the rim at angle `a`.
fn sweep_arm_path(a: f32) -> String {
    format!(
        "M {RADAR_C} {RADAR_C} L {:.1} {:.1}",
        RADAR_C + a.cos() * RADAR_R,
        RADAR_C + a.sin() * RADAR_R
    )
}

/// The faded pie wedge trailing behind the sweep arm.
fn sweep_wedge_path(a: f32) -> String {
    format!(
        "M {RADAR_C} {RADAR_C} L {:.1} {:.1} A {RADAR_R} {RADAR_R} 0 0 1 {:.1} {:.1} Z",
        RADAR_C + (a - RADAR_WEDGE_SPAN).cos() * RADAR_R,
        RADAR_C + (a - RADAR_WEDGE_SPAN).sin() * RADAR_R,
        RADAR_C + a.cos() * RADAR_R,
        RADAR_C + a.sin() * RADAR_R
    )
}

/// Expanding pulse ring at progress `t` (0..1): returns (diameter px, opacity).
/// Rings start small and bright, growing and fading out; voice energy makes
/// them brighter.
fn pulse_ring(t: f32, level: f32) -> (f32, f32) {
    (22.0 + t * 96.0, (1.0 - t) * (0.4 + level * 0.6))
}

/// Contact blips flash as the sweep passes their bearing, then fade until the
/// next revolution.
fn blip_opacity(sweep_angle: f32, bearing: f32) -> f32 {
    let d = (sweep_angle - bearing).rem_euclid(std::f32::consts::TAU);
    (1.0 - d / 2.2).clamp(0.0, 1.0)
}

// ── Ocean (blue_wave style) ──────────────────────────────────────────
const OCEAN_H: f32 = 90.0; // wave stage height

/// Remap a waterline offset by the load/unload progress `fill` (0..1):
/// at fill=1 the water is at its natural level, at fill=0 it has fully
/// drained below the bottom edge.
fn drain_level(y_off: f32, fill: f32) -> f32 {
    OCEAN_H - (OCEAN_H - y_off) * fill
}

// ── VU dot matrix (voice_card style) ─────────────────────────────────

/// Target level for LED column `i` of `n`. `noise` is a random value in 0..1.
fn led_column_target(
    i: usize,
    n: usize,
    level: f32,
    phase: f32,
    recording: bool,
    processing: bool,
    ready: bool,
    noise: f32,
) -> f32 {
    if processing {
        0.25 + 0.75 * ((phase * 4.0 - i as f32 * 0.45).sin()).max(0.0)
    } else if recording && !ready {
        0.08 + 0.06 * noise
    } else {
        let mid = (n as f32 - 1.0) / 2.0;
        let env = (-((i as f32 - mid).powi(2)) / 60.0).exp();
        // sqrt curve so quiet speech still lights the meter
        (level.sqrt() * (0.6 + 0.6 * noise) * env * 1.6).min(1.0)
    }
}

/// VU-meter ballistics: jump instantly on attack, decay slowly on release.
fn led_step(current: f32, target: f32) -> f32 {
    if target > current { target } else { current * 0.86 }
}

// ── Mono bars (mono_bars style) ───────────────────────────────────────
const MONO_BAR_MIN: f32 = 8.0;
const MONO_BAR_MAX: f32 = 52.0;

/// Bar height in px for bar `i` of `n` (always within [MONO_BAR_MIN,
/// MONO_BAR_MAX]). Bars are centre-weighted and ripple gently across the
/// row while recording, in lock-step while processing, and sit flat at
/// the baseline otherwise.
fn mono_bar_height(i: usize, n: usize, level: f32, phase: f32, recording: bool, processing: bool, ready: bool, noise: f32) -> f32 {
    let mid = (n as f32 - 1.0) / 2.0;
    let envelope = 1.0 - (i as f32 - mid).abs() / (mid + 1.0) * 0.4;
    let amp = if processing {
        ((phase * 4.0 - i as f32 * 0.85).sin() * 0.5 + 0.5) * envelope
    } else if recording && !ready {
        0.0
    } else if recording {
        let ripple = (phase * 3.0 - i as f32 * 0.8).sin();
        (level * envelope * (0.9 + 0.1 * ripple) * (0.85 + 0.3 * noise)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    MONO_BAR_MIN + (MONO_BAR_MAX - MONO_BAR_MIN) * amp
}

// ── Neon spectrum (spectrum style) ──────────────────────────────────────
const SPECTRUM_BARS: usize = 16;
const SPECTRUM_MIN: f32 = 6.0;
const SPECTRUM_MAX: f32 = 96.0;

/// Bar height in px for band `i` of `n` (always within [SPECTRUM_MIN,
/// SPECTRUM_MAX]). Lower bands ("bass") swing wider and slower; higher
/// bands ("treble") flicker faster with a smaller share of the level.
fn spectrum_bar_height(i: usize, n: usize, level: f32, phase: f32, recording: bool, processing: bool, ready: bool, noise: f32) -> f32 {
    let band = i as f32 / (n as f32 - 1.0);
    let amp = if processing {
        ((phase * 2.4 - band * 6.0).sin() * 0.5 + 0.5).powf(1.5)
    } else if recording && !ready {
        noise * 0.06
    } else if recording {
        let band_freq = 2.0 + band * 9.0;
        let band_phase = i as f32 * 0.7;
        let wobble = (phase * band_freq + band_phase).sin() * 0.5 + 0.5;
        let band_gain = 1.0 - band * 0.35;
        (level * band_gain * (0.35 + 0.65 * wobble) * (0.7 + 0.6 * noise)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    SPECTRUM_MIN + (SPECTRUM_MAX - SPECTRUM_MIN) * amp
}

// ── Retro terminal (terminal style) ─────────────────────────────────────
const ASCII_METER_WIDTH: usize = 20;

/// Build a fixed-width block-character meter for the terminal's level
/// readout. While processing it shows a scanning block bouncing back and
/// forth; while recording it fills left-to-right with the audio level.
fn ascii_meter(level: f32, phase: f32, recording: bool, processing: bool, ready: bool) -> String {
    let w = ASCII_METER_WIDTH;
    if processing {
        let cycle = 2 * w;
        let raw = (phase * 10.0) as usize % cycle;
        let pos = if raw < w { raw } else { cycle - 1 - raw };
        (0..w).map(|i| if i == pos { '█' } else { '·' }).collect()
    } else if recording && !ready {
        (0..w).map(|i| if i % 4 == 0 { '·' } else { ' ' }).collect()
    } else if recording {
        let filled = (level.clamp(0.0, 1.0) * w as f32).round() as usize;
        (0..w).map(|i| if i < filled { '█' } else { '·' }).collect()
    } else {
        "·".repeat(w)
    }
}

// ── Analog VU (vinyl style) ──────────────────────────────────────────────
const VU_PIVOT_X: f32 = 144.0;
const VU_PIVOT_Y: f32 = 70.0;
const VU_RADIUS: f32 = 58.0;
const VU_ANGLE_MIN: f32 = -0.95; // resting position (full left)
const VU_ANGLE_MAX: f32 = 0.95; // full-scale (full right)

/// Map a 0..1 audio level to a needle angle (radians from straight up).
fn vu_target_angle(level: f32) -> f32 {
    VU_ANGLE_MIN + level.clamp(0.0, 1.0) * (VU_ANGLE_MAX - VU_ANGLE_MIN)
}

/// The needle: a line from the pivot to a point on the dial rim at `angle`
/// radians from straight up.
fn vu_needle_path(angle: f32) -> String {
    format!(
        "M {VU_PIVOT_X} {VU_PIVOT_Y} L {:.1} {:.1}",
        VU_PIVOT_X + angle.sin() * VU_RADIUS,
        VU_PIVOT_Y - angle.cos() * VU_RADIUS
    )
}

// ── Aurora ribbon (aurora style) ──────────────────────────────────────────
const AURORA_MID: f32 = 39.0;

/// A flowing double-sine ribbon trace across the panel. Louder voice widens
/// the swing; processing slows and smooths it into a gentle drift.
fn aurora_path(width: f32, level: f32, phase: f32, processing: bool, ready: bool) -> String {
    let amp = if processing {
        10.0 + 4.0 * (phase * 0.9).sin()
    } else if !ready {
        3.0
    } else {
        6.0 + level * 24.0
    };
    let mut d = String::with_capacity(1024);
    let mut x = 0.0_f32;
    while x <= width {
        let y = AURORA_MID
            + (x * 0.018 + phase * 1.6).sin() * amp
            + (x * 0.031 - phase * 0.9).sin() * amp * 0.35;
        if x == 0.0 {
            let _ = write!(d, "M {x:.0} {y:.1}");
        } else {
            let _ = write!(d, " L {x:.0} {y:.1}");
        }
        x += 6.0;
    }
    d
}

// ── Holo ring (holo style) ────────────────────────────────────────────────
const HOLO_CX: f32 = 90.0;
const HOLO_CY: f32 = 90.0;
const HOLO_RX: f32 = 60.0;
const HOLO_RY: f32 = 34.0;
const HOLO_NODE_ANGLES: [f32; 6] = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0];

/// One orbital ring as a rotated ellipse, traced as a closed Bezier path
/// around (HOLO_CX, HOLO_CY), offset by `rotation_deg`.
fn holo_ring_path(rotation_deg: f32) -> String {
    let rot = rotation_deg.to_radians();
    let (cr, sr) = (rot.cos(), rot.sin());
    // Sample the unrotated ellipse and rotate each point about the centre.
    let pt = |t: f32| -> (f32, f32) {
        let ex = HOLO_RX * t.cos();
        let ey = HOLO_RY * t.sin();
        (HOLO_CX + ex * cr - ey * sr, HOLO_CY + ex * sr + ey * cr)
    };
    let steps = 24;
    let mut d = String::with_capacity(512);
    for i in 0..=steps {
        let t = i as f32 / steps as f32 * std::f32::consts::TAU;
        let (x, y) = pt(t);
        if i == 0 {
            let _ = write!(d, "M {x:.1} {y:.1}");
        } else {
            let _ = write!(d, " L {x:.1} {y:.1}");
        }
    }
    let _ = write!(d, " Z");
    d
}

/// Position of particle node `node_angle_deg` on the rim, rotated by the
/// current sweep `rotation_deg`.
fn holo_node_pos(rotation_deg: f32, node_angle_deg: f32) -> (f32, f32) {
    let t = (node_angle_deg + rotation_deg).to_radians();
    (HOLO_CX + t.cos() * HOLO_RX, HOLO_CY + t.sin() * HOLO_RY)
}

/// Particle nodes flash as the sweep passes their bearing, then fade until
/// the next revolution (mirrors `blip_opacity`).
fn holo_node_opacity(rotation_deg: f32, node_angle_deg: f32) -> f32 {
    let d = (rotation_deg - node_angle_deg).rem_euclid(360.0);
    (1.0 - d / 90.0).clamp(0.15, 1.0)
}

// ── Frequency petals (petals style) ───────────────────────────────────────
const PETAL_COUNT: usize = 24;
const PETAL_MAX: f32 = 58.0;
const PETAL_CX: f32 = 85.0;
const PETAL_CY: f32 = 85.0;
const PETAL_HALF_W: f32 = 2.5;

/// Spoke length in px for petal `i` of `n`, blooming outward with voice
/// energy (mirrors `spectrum_bar_height`'s radial envelope shape).
fn petal_length(i: usize, n: usize, level: f32, phase: f32, recording: bool, processing: bool, ready: bool, noise: f32) -> f32 {
    let band = i as f32 / n as f32;
    let amp = if processing {
        ((phase * 2.2 + band * std::f32::consts::TAU).sin() * 0.5 + 0.5).powf(1.4)
    } else if recording && !ready {
        0.04 + 0.03 * noise
    } else if recording {
        let wobble = (phase * 3.4 + band * 7.0).sin() * 0.5 + 0.5;
        (level * (0.5 + 0.5 * wobble) * (0.75 + 0.5 * noise)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    14.0 + (PETAL_MAX - 14.0) * amp
}

/// One spoke as a rounded capsule path, rotated by `angle_deg`.
fn petal_spoke_path(angle_deg: f32, length: f32) -> String {
    let a = angle_deg.to_radians();
    let (c, s) = (a.cos(), a.sin());
    let rotate = |x: f32, y: f32| -> (f32, f32) { (PETAL_CX + x * c - y * s, PETAL_CY + x * s + y * c) };
    let (x0, y0) = rotate(0.0, PETAL_HALF_W);
    let (x1, y1) = rotate(length, PETAL_HALF_W);
    let (x2, y2) = rotate(length, -PETAL_HALF_W);
    let (x3, y3) = rotate(0.0, -PETAL_HALF_W);
    format!(
        "M {x0:.1} {y0:.1} L {x1:.1} {y1:.1} A {PETAL_HALF_W} {PETAL_HALF_W} 0 0 1 {x2:.1} {y2:.1} L {x3:.1} {y3:.1} Z"
    )
}

/// Build the combined multi-subpath string for every third spoke starting
/// at `offset` (used to split the spokes into three colour tiers).
fn petal_group_path(lengths: &[f32], offset: usize) -> String {
    let n = lengths.len();
    let mut d = String::with_capacity(lengths.len() * 80);
    for i in (offset..n).step_by(3) {
        d.push_str(&petal_spoke_path(i as f32 * (360.0 / n as f32), lengths[i]));
        d.push(' ');
    }
    d
}

// ── Minimal dot strip (dotstrip style) ────────────────────────────────────
const DOTSTRIP_DOTS: usize = 9;

/// Scale factor (roughly 1.0..2.2) for dot `i` of `n`, lighting up
/// centre-out with voice energy (mirrors `mono_bar_height`'s envelope).
fn dotstrip_scale(i: usize, n: usize, level: f32, phase: f32, recording: bool, processing: bool, ready: bool, noise: f32) -> f32 {
    let mid = (n as f32 - 1.0) / 2.0;
    let envelope = 1.0 - (i as f32 - mid).abs() / (mid + 1.0) * 0.5;
    let amp = if processing {
        ((phase * 4.0 - i as f32 * 0.7).sin() * 0.5 + 0.5) * envelope
    } else if recording && !ready {
        0.05
    } else if recording {
        let ripple = (phase * 3.2 - i as f32 * 0.6).sin();
        (level * envelope * (0.9 + 0.1 * ripple) * (0.85 + 0.3 * noise)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    1.0 + amp * 1.2
}

// ── Starfield spectrum (starfield style) ──────────────────────────────────
const STARFIELD_STAR_COUNT: usize = 36;
const STARFIELD_W: f32 = 504.0;
const STARFIELD_H: f32 = 126.0;
const STARFIELD_MID: f32 = 74.0;
const STARFIELD_AMP: f32 = 26.0;

/// Deterministic fixed star layout, generated once at startup with a tiny
/// LCG so the field doesn't reshuffle every frame.
fn build_starfield(seed: &mut u32) -> Vec<StarData> {
    let mut next = || -> f32 {
        *seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        (*seed >> 8) as f32 / ((u32::MAX >> 8) as f32)
    };
    (0..STARFIELD_STAR_COUNT)
        .map(|_| StarData {
            x: next() * STARFIELD_W,
            y: 14.0 + next() * (STARFIELD_H - 24.0),
            r: 1.0 + next() * 1.8,
            o: 0.15 + next() * 0.35,
        })
        .collect()
}

/// Twinkle opacity for a star with a fixed per-star `phase_offset`: a slow
/// shimmer that brightens with voice energy.
fn star_twinkle(base_o: f32, level: f32, phase: f32, phase_offset: f32) -> f32 {
    let shimmer = (phase * 1.3 + phase_offset).sin() * 0.5 + 0.5;
    (base_o + shimmer * 0.25 + level * 0.3).clamp(0.0, 1.0)
}

/// Render the level-history ring buffer as a connected thread across the
/// starfield panel (mirrors `build_osc_path`).
fn starfield_path(hist: &VecDeque<f32>) -> String {
    let mut d = String::with_capacity(2048);
    for (i, v) in hist.iter().enumerate() {
        let x = i as f32 * 4.0;
        let y = (STARFIELD_MID - v * STARFIELD_AMP).clamp(4.0, STARFIELD_H - 4.0);
        if i == 0 {
            let _ = write!(d, "M {x:.0} {y:.1}");
        } else {
            let _ = write!(d, " L {x:.0} {y:.1}");
        }
    }
    d
}

fn main() {
    let ui = OverlayWindow::new().unwrap();

    // Start with the window hidden. We show()/hide() dynamically.

    let shared_state = Arc::new(Mutex::new(AppState {
        recording: false,
        processing: false,
        speaking: false,
        audio_ready: true,
        audio_level: 0.0,
        active_target_label: "Focused Window".to_string(),
        x: -20000,
        y: -20000,
        overlay_style: "waveform".to_string(),
    }));

    // Stdin reading thread
    let state_clone = Arc::clone(&shared_state);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // Clean EOF: the parent closed our stdin, so shut down.
                    eprintln!("[overlay] stdin closed (EOF); exiting event loop");
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                    break;
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                    // Transient interruption (e.g. EINTR): keep reading instead
                    // of tearing down the overlay for the rest of the session.
                    eprintln!("[overlay] stdin read interrupted, retrying: {e}");
                    continue;
                }
                Err(e) => {
                    eprintln!("[overlay] stdin read error; exiting event loop: {e}");
                    let _ = slint::invoke_from_event_loop(|| {
                        let _ = slint::quit_event_loop();
                    });
                    break;
                }
            }

            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(msg_type) = msg.get("type").and_then(|t| t.as_str()) {
                    match msg_type {
                        "status" => {
                            if let Ok(mut state) = state_clone.lock() {
                                state.recording = msg.get("recording").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.processing = msg.get("processing").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.speaking = msg.get("speaking").and_then(|v| v.as_bool()).unwrap_or(false);
                                state.audio_ready = msg.get("audio_ready").and_then(|v| v.as_bool()).unwrap_or(true);
                                state.audio_level = msg.get("audio_level").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                                if let Some(target) = msg.get("active_target_label").and_then(|v| v.as_str()) {
                                    state.active_target_label = target.to_string();
                                }
                                if let Some(style) = msg.get("overlay_style").and_then(|v| v.as_str()) {
                                    state.overlay_style = style.to_string();
                                }
                            }
                        }
                        "position" => {
                            if let Ok(mut state) = state_clone.lock() {
                                state.x = msg.get("x").and_then(|v| v.as_i64()).unwrap_or(-20000) as i32;
                                state.y = msg.get("y").and_then(|v| v.as_i64()).unwrap_or(-20000) as i32;
                            }
                        }
                        "shutdown" => {
                            slint::invoke_from_event_loop(move || {
                                let _ = slint::quit_event_loop();
                            }).unwrap();
                            break;
                        }
                        _ => {}
                    }
                }
            }
        }
    });

    let timer = slint::Timer::default();
    let state_clone2 = Arc::clone(&shared_state);
    let ui_weak = ui.as_weak();

    // ── Animation state owned by the render tick ────────────────────
    let mut reveal_main = 0.0_f32;
    let mut vel_main = 0.0_f32;
    let mut reveal_pill = 0.0_f32;
    let mut vel_pill = 0.0_f32;
    let mut phase = 0.0_f32;
    let mut cur_level = 0.0_f32;
    let mut osc_hist: VecDeque<f32> = VecDeque::from(vec![0.0_f32; 126]);
    let mut led_cols = [0.0_f32; 20];
    let mut vu_angle = VU_ANGLE_MIN;
    let mut vu_vel = 0.0_f32;
    let mut lcg_state = 12345_u32;
    let mut shown = false;
    let mut idle = false;
    let mut holo_rotation = 0.0_f32;
    let mut starfield_hist: VecDeque<f32> = VecDeque::from(vec![0.0_f32; 126]);
    let mut starfield_seed = 99991_u32;
    let stars = build_starfield(&mut starfield_seed);
    let star_offsets: Vec<f32> = (0..stars.len()).map(|i| i as f32 * 0.71).collect();

    const DT: f32 = 0.016;

    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(16), move || {
        let ui = match ui_weak.upgrade() {
            Some(ui) => ui,
            None => return,
        };

        let (rec, proc, speak, ready, raw_level, target, tx, ty, style) = {
            if let Ok(s) = state_clone2.lock() {
                (s.recording, s.processing, s.speaking, s.audio_ready, s.audio_level, s.active_target_label.clone(), s.x, s.y, s.overlay_style.clone())
            } else {
                return;
            }
        };

        // ── Animation clocks ─────────────────────────────────────────
        phase += DT;
        if phase > 100_000.0 {
            phase = 0.0;
        }
        let active_main = rec || proc;
        let active_pill = speak;
        spring(&mut reveal_main, &mut vel_main, if active_main { 1.0 } else { 0.0 }, DT);
        spring(&mut reveal_pill, &mut vel_pill, if active_pill { 1.0 } else { 0.0 }, DT);
        reveal_main = reveal_main.max(0.0);
        reveal_pill = reveal_pill.max(0.0);

        // Smoothed, normalized audio level (fast attack, fast release)
        let tgt_level = (raw_level * 100.0).clamp(0.0, 1.0);
        cur_level += (tgt_level - cur_level) * 0.35;

        let mut rand = || {
            lcg_state = lcg_state.wrapping_mul(1103515245).wrapping_add(12345);
            (lcg_state >> 8) as f32 / ((u32::MAX >> 8) as f32)
        };

        // ── Window mapping ──────────────────────────────────────────────
        // Map the overlay lazily the first time it is actually needed, then
        // keep it mapped for the lifetime of the process. On Wayland,
        // hide()/show() re-maps the surface and the compositor (KWin) grabs
        // keyboard focus for the freshly-mapped overlay on every dictation —
        // stealing focus from the user's window so injected text never reaches
        // the cursor. Mapping once (with real content) and never unmapping
        // avoids the per-dictation focus grab; when idle we simply render a
        // transparent frame instead of hiding.
        let visible_needed = active_main || active_pill || reveal_main > 0.004 || reveal_pill > 0.004;

        if visible_needed && !shown {
            if let Err(e) = ui.show() {
                eprintln!("[overlay] Failed to show window: {:?}", e);
            }
            ui.window().with_winit_window(|w| {
                if let Err(e) = w.set_cursor_hittest(false) {
                    eprintln!("[overlay] Failed to make window click-through: {:?}", e);
                }
                w.set_window_level(i_slint_backend_winit::winit::window::WindowLevel::AlwaysOnTop);
            });
            shown = true;
            eprintln!("[overlay] window mapped (kept mapped for the session)");
        }

        // Nothing has needed the overlay yet — leave it unmapped.
        if !shown {
            return;
        }

        // Mapped but idle: keep the window invisible (reveal == 0 draws
        // nothing) but DON'T stop driving the surface. On Wayland, a window
        // that stops drawing entirely also stops receiving frame callbacks from
        // some compositors (Mutter/KWin) — so when the next dictation arrives,
        // Slint marks the window dirty but the compositor never delivers a
        // frame and the overlay is never re-presented. The result is the
        // overlay showing exactly once per session. Nudging winit to redraw on
        // every idle tick keeps the frame-callback loop alive; the idle frame
        // is essentially free since nothing is drawn.
        if !visible_needed {
            if !idle {
                ui.set_reveal_main(0.0);
                ui.set_reveal_pill(0.0);
                ui.set_level(0.0);
                idle = true;
            }
            ui.window().with_winit_window(|w| w.request_redraw());
            return;
        }
        idle = false;

        if tx > -10000 {
            ui.window().set_position(slint::PhysicalPosition::new(tx, ty));
        }

        // ── Shared status properties ────────────────────────────────
        ui.set_is_recording(rec);
        ui.set_is_processing(proc);
        ui.set_is_speaking(speak);
        ui.set_is_audio_ready(ready);
        ui.set_target_label(target.into());
        ui.set_overlay_style(style.clone().into());
        ui.set_reveal_main(reveal_main.min(1.12));
        ui.set_reveal_pill(reveal_pill.min(1.12));
        ui.set_level(cur_level);
        ui.set_blink(0.5 + 0.5 * (phase * 5.5).sin());

        // ── Per-style visualizer data ────────────────────────────────
        match style.as_str() {
            // Oscilloscope: scroll a live trace through a ring buffer.
            "waveform" => {
                let sample = osc_sample(rec, proc, ready, cur_level, phase, rand() * 2.0 - 1.0);
                osc_hist.pop_front();
                osc_hist.push_back(sample);
                ui.set_osc_path(build_osc_path(&osc_hist).into());
            }

            // Radar: sweep arm, trail, expanding pulse rings and blips.
            "pulse" => {
                let a = phase * 4.2;
                ui.set_sweep_path(sweep_arm_path(a).into());
                ui.set_sweep_trail_path(sweep_wedge_path(a).into());

                // Pulse rings emitted from the core; louder voice = brighter rings.
                let (s1, o1) = pulse_ring((phase * 0.75).fract(), cur_level);
                let (s2, o2) = pulse_ring((phase * 0.75 + 0.5).fract(), cur_level);
                ui.set_ring1_s(s1);
                ui.set_ring1_o(o1);
                ui.set_ring2_s(s2);
                ui.set_ring2_o(o2);

                // Blips flash as the sweep passes their bearing.
                ui.set_blip1_o(blip_opacity(a, -0.35));
                ui.set_blip2_o(blip_opacity(a, 2.45));
            }

            // Ocean: three layered waves whose tide rises with the voice.
            // reveal-main scales the fill so water drains out on unload.
            "blue_wave" => {
                let fill = reveal_main.min(1.0);
                let lift = cur_level;
                let proc_surge = if proc { 4.0 + 2.5 * (phase * 1.6).sin() } else { 0.0 };

                let y1 = 64.0 - lift * 20.0 - proc_surge;
                let y2 = 56.0 - lift * 24.0 - proc_surge * 1.2;
                let y3 = 47.0 - lift * 28.0 - proc_surge * 1.4;

                let a1 = (if proc { 5.0 } else { 2.5 }) + lift * 14.0;
                let a2 = (if proc { 6.0 } else { 2.0 }) + lift * 18.0;
                let a3 = (if proc { 7.0 } else { 1.5 }) + lift * 22.0;

                let y3_final = drain_level(y3, fill);
                ui.set_ocean_path1(ocean_wave_path(380.0, OCEAN_H, a1, 0.014, phase * 2.1, drain_level(y1, fill)).into());
                ui.set_ocean_path2(ocean_wave_path(380.0, OCEAN_H, a2, 0.022, -phase * 3.0, drain_level(y2, fill)).into());
                ui.set_ocean_path3(ocean_wave_path(380.0, OCEAN_H, a3, 0.018, phase * 3.9, y3_final).into());

                // Buoy sits on the front wave's surface (panel coords),
                // bobbing with the swell.
                ui.set_buoy_y(38.0 + y3_final - 21.0 + (phase * 2.2).sin() * 2.5);

                // Bubbles rise faster while there is voice energy.
                let bubbles = std::rc::Rc::new(slint::VecModel::default());
                for i in 0..3 {
                    let speed = 14.0 + cur_level * 26.0;
                    let yy = 84.0 - (phase * speed + i as f32 * 31.0).rem_euclid(78.0);
                    let o = (0.25 + (yy / 84.0) * 0.45).min(0.65) * fill;
                    bubbles.push(BubbleData { y: 38.0 + yy, o });
                }
                ui.set_bubbles(bubbles.into());
            }

            // Voice card: VU dot-matrix column levels with peak decay.
            "voice_card" => {
                let n = led_cols.len();
                for i in 0..n {
                    let target = led_column_target(i, n, cur_level, phase, rec, proc, ready, rand());
                    led_cols[i] = led_step(led_cols[i], target);
                }
                let cols = std::rc::Rc::new(slint::VecModel::default());
                for c in led_cols.iter() {
                    cols.push(*c);
                }
                ui.set_led_cols(cols.into());
            }

            // Mono bars: hyper-minimal 5-bar black & white level meter.
            "mono_bars" => {
                let n = 5;
                let bars = std::rc::Rc::new(slint::VecModel::default());
                for i in 0..n {
                    bars.push(mono_bar_height(i, n, cur_level, phase, rec, proc, ready, rand()));
                }
                ui.set_mono_bars(bars.into());
            }

            // Neon spectrum: 16-band magenta-to-cyan equalizer.
            "spectrum" => {
                let n = SPECTRUM_BARS;
                let bars = std::rc::Rc::new(slint::VecModel::default());
                for i in 0..n {
                    bars.push(spectrum_bar_height(i, n, cur_level, phase, rec, proc, ready, rand()));
                }
                ui.set_spectrum_bars(bars.into());
            }

            // Retro terminal: block-character ASCII level meter.
            "terminal" => {
                ui.set_ascii_meter(ascii_meter(cur_level, phase, rec, proc, ready).into());
            }

            // Analog VU: spring-loaded needle kicks toward the level and
            // settles back to rest when idle.
            "vinyl" => {
                let target_angle = if proc {
                    vu_target_angle(0.3 + 0.25 * (phase * 2.0).sin().abs())
                } else if rec && ready {
                    vu_target_angle(cur_level)
                } else {
                    VU_ANGLE_MIN
                };
                spring(&mut vu_angle, &mut vu_vel, target_angle, DT);
                ui.set_vu_needle_path(vu_needle_path(vu_angle).into());
            }

            // Aurora ribbon: a flowing sine trace, glow + tri-tone passes
            // all share the same underlying path.
            "aurora" => {
                ui.set_aurora_path(aurora_path(504.0, cur_level, phase, proc, ready).into());
            }

            // Holo ring: a steadily rotating sweep with orbital ring traces
            // and particle nodes that flash as the sweep passes.
            "holo" => {
                let speed = if proc { 140.0 } else if rec && ready { 90.0 + cur_level * 120.0 } else { 30.0 };
                holo_rotation = (holo_rotation + speed * DT).rem_euclid(360.0);
                ui.set_holo_ring1_path(holo_ring_path(holo_rotation).into());
                ui.set_holo_ring2_path(holo_ring_path(holo_rotation + 60.0).into());
                ui.set_holo_ring3_path(holo_ring_path(holo_rotation + 120.0).into());

                let nodes = std::rc::Rc::new(slint::VecModel::default());
                for &na in HOLO_NODE_ANGLES.iter() {
                    let (x, y) = holo_node_pos(holo_rotation, na);
                    let o = holo_node_opacity(holo_rotation, na);
                    nodes.push(HoloNode { x, y, o });
                }
                ui.set_holo_nodes(nodes.into());
            }

            // Frequency petals: a radial mandala of spokes blooming
            // outward, split into three colour tiers.
            "petals" => {
                let n = PETAL_COUNT;
                let lengths: Vec<f32> = (0..n)
                    .map(|i| petal_length(i, n, cur_level, phase, rec, proc, ready, rand()))
                    .collect();
                ui.set_petal_path_a(petal_group_path(&lengths, 0).into());
                ui.set_petal_path_b(petal_group_path(&lengths, 1).into());
                ui.set_petal_path_c(petal_group_path(&lengths, 2).into());
            }

            // Minimal dot strip: a compact row of dots lighting up
            // centre-out with the voice level.
            "dotstrip" => {
                let n = DOTSTRIP_DOTS;
                let dots = std::rc::Rc::new(slint::VecModel::default());
                for i in 0..n {
                    dots.push(dotstrip_scale(i, n, cur_level, phase, rec, proc, ready, rand()));
                }
                ui.set_dot_bars(dots.into());
            }

            // Starfield spectrum: a fixed constellation with a connected
            // level-history thread running through it.
            "starfield" => {
                let sample = osc_sample(rec, proc, ready, cur_level, phase, rand() * 2.0 - 1.0);
                starfield_hist.pop_front();
                starfield_hist.push_back(sample);
                ui.set_starfield_path(starfield_path(&starfield_hist).into());

                let live_stars = std::rc::Rc::new(slint::VecModel::default());
                for (st, &offset) in stars.iter().zip(star_offsets.iter()) {
                    live_stars.push(StarData {
                        x: st.x,
                        y: st.y,
                        r: st.r,
                        o: star_twinkle(st.o, cur_level, phase, offset),
                    });
                }
                ui.set_stars(live_stars.into());
            }

            _ => {}
        }

        // Holo sheen keeps drifting across the voice card.
        if style == "voice_card" {
            ui.set_sheen_x((phase * 70.0).rem_euclid(480.0) - 70.0);
        }

        // Speaking pill mini-equalizer.
        if reveal_pill > 0.004 {
            let bars = std::rc::Rc::new(slint::VecModel::default());
            for i in 0..5 {
                bars.push(8.0_f32 + 13.0 * (0.5 + 0.5 * (phase * 8.0 + i as f32 * 1.05).sin()));
            }
            ui.set_pill_bars(bars.into());
        }
    });

    if let Err(e) = slint::run_event_loop_until_quit() {
        eprintln!("[overlay] event loop ended with error: {e}");
    } else {
        eprintln!("[overlay] event loop ended");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 0.016;

    // ── Load/unload spring ───────────────────────────────────────────

    #[test]
    fn spring_converges_to_target_and_settles() {
        let (mut x, mut v) = (0.0_f32, 0.0_f32);
        for _ in 0..120 {
            spring(&mut x, &mut v, 1.0, DT);
        }
        assert_eq!(x, 1.0, "reveal must settle exactly on the target");
        assert_eq!(v, 0.0, "velocity must be zeroed once settled");
    }

    #[test]
    fn spring_unload_returns_to_zero() {
        let (mut x, mut v) = (1.0_f32, 0.0_f32);
        for _ in 0..120 {
            spring(&mut x, &mut v, 0.0, DT);
        }
        assert_eq!(x, 0.0);
    }

    #[test]
    fn spring_overshoot_is_bounded() {
        // Slight overshoot is intentional, but reveal drives widths/heights
        // so it must stay well-behaved.
        let (mut x, mut v) = (0.0_f32, 0.0_f32);
        let mut peak = 0.0_f32;
        for _ in 0..240 {
            spring(&mut x, &mut v, 1.0, DT);
            peak = peak.max(x);
            assert!(x.is_finite() && v.is_finite());
        }
        assert!(peak > 1.0, "spring should overshoot slightly for the bounce");
        assert!(peak < 1.15, "overshoot must stay subtle, got {peak}");
    }

    #[test]
    fn spring_reaches_visibility_threshold_quickly() {
        // The window is hidden once reveal falls below 0.004; the unload
        // animation must complete in well under a second.
        let (mut x, mut v) = (1.0_f32, 0.0_f32);
        let mut ticks = 0;
        while x > 0.004 && ticks < 625 {
            spring(&mut x, &mut v, 0.0, DT);
            ticks += 1;
        }
        assert!(
            (ticks as f32) * DT < 1.0,
            "unload animation took too long: {}s",
            ticks as f32 * DT
        );
    }

    // ── Oscilloscope (waveform) ──────────────────────────────────────

    #[test]
    fn osc_trace_is_right_side_up() {
        // A positive sample must deflect the trace upwards (smaller y).
        let mut hist: VecDeque<f32> = VecDeque::from(vec![0.0, 1.0]);
        let path = build_osc_path(&hist);
        let ys: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .skip(1) // first parsed number is x of the first point
            .step_by(2)
            .collect();
        assert_eq!(ys.len(), 2);
        assert!(ys[1] < ys[0], "positive sample must move the trace up: {path}");

        // And a negative sample deflects downwards.
        hist[1] = -1.0;
        let path = build_osc_path(&hist);
        let ys: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .skip(1)
            .step_by(2)
            .collect();
        assert!(ys[1] > ys[0], "negative sample must move the trace down: {path}");
    }

    #[test]
    fn osc_trace_stays_inside_the_scope_stage() {
        // Even absurd samples must clamp inside the 78px stage.
        let hist: VecDeque<f32> = VecDeque::from(vec![0.0, 5.0, -5.0, 1.0, -1.0]);
        let path = build_osc_path(&hist);
        let nums: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        for y in nums.iter().skip(1).step_by(2) {
            assert!((2.0..=76.0).contains(y), "trace escaped the stage: y={y}");
        }
    }

    #[test]
    fn osc_path_has_one_point_per_sample() {
        let hist: VecDeque<f32> = VecDeque::from(vec![0.0; 126]);
        let path = build_osc_path(&hist);
        assert!(path.starts_with("M "));
        assert_eq!(path.matches("L ").count(), 125);
    }

    #[test]
    fn osc_sample_is_flat_when_idle_and_bounded_when_active() {
        assert_eq!(osc_sample(false, false, true, 1.0, 3.0, 1.0), 0.0);
        for i in 0..200 {
            let phase = i as f32 * 0.016;
            let noise = if i % 2 == 0 { 1.0 } else { -1.0 };
            let s = osc_sample(true, false, true, 1.0, phase, noise);
            assert!(s.abs() <= 1.0, "sample out of range: {s}");
        }
    }

    // ── Radar (pulse) ────────────────────────────────────────────────

    #[test]
    fn sweep_arm_rotates_and_stays_on_the_dial() {
        let p1 = sweep_arm_path(0.0);
        let p2 = sweep_arm_path(1.0);
        assert_ne!(p1, p2, "sweep arm must move as the angle advances");

        for a in [0.0_f32, 1.0, 2.5, 4.0, 6.0] {
            let path = sweep_arm_path(a);
            let nums: Vec<f32> = path
                .split_whitespace()
                .filter_map(|t| t.parse::<f32>().ok())
                .collect();
            let (x, y) = (nums[2], nums[3]);
            let dist = ((x - RADAR_C).powi(2) + (y - RADAR_C).powi(2)).sqrt();
            assert!(
                (dist - RADAR_R).abs() < 0.2,
                "arm tip must sit on the sweep radius, got {dist}"
            );
        }
    }

    #[test]
    fn sweep_wedge_is_a_closed_arc_segment() {
        let path = sweep_wedge_path(1.2);
        assert!(path.starts_with(&format!("M {RADAR_C} {RADAR_C}")));
        assert!(path.contains(" A "), "wedge must contain an arc command");
        assert!(path.ends_with('Z'), "wedge must be a closed (fillable) shape");
    }

    #[test]
    fn pulse_rings_grow_and_fade() {
        let (s0, o0) = pulse_ring(0.0, 0.5);
        let (s1, o1) = pulse_ring(1.0, 0.5);
        assert!(s1 > s0, "ring must expand over its lifetime");
        assert!(o0 > 0.0 && o1 == 0.0, "ring must fade out completely");

        // Louder voice means brighter rings, but opacity stays valid.
        let (_, quiet) = pulse_ring(0.2, 0.0);
        let (_, loud) = pulse_ring(0.2, 1.0);
        assert!(loud > quiet);
        assert!((0.0..=1.0).contains(&loud));
    }

    #[test]
    fn blips_flash_on_sweep_pass_and_fade_after() {
        let bearing = 1.0_f32;
        assert!((blip_opacity(bearing, bearing) - 1.0).abs() < 1e-6);
        let just_after = blip_opacity(bearing + 0.5, bearing);
        let long_after = blip_opacity(bearing + 2.0, bearing);
        assert!(just_after > long_after);
        assert_eq!(blip_opacity(bearing + 3.0, bearing), 0.0);
        // Approaching (not yet passed) blips are dark: the distance wraps.
        assert_eq!(blip_opacity(bearing - 0.5, bearing), 0.0);
    }

    // ── Ocean (blue_wave) ────────────────────────────────────────────

    #[test]
    fn ocean_bottom_is_locked_to_the_stage_bottom() {
        // Regression: the wave shape must always close along y = OCEAN_H so
        // the bottom of the water never moves while animating.
        for phase in [0.0_f32, 1.3, 2.7, 9.9] {
            let path = ocean_wave_path(380.0, OCEAN_H, 22.0, 0.018, phase, 19.0);
            assert!(path.starts_with("M 0 90"), "must start at the bottom-left: {path}");
            assert!(path.ends_with("L 380 90 Z"), "must close at the bottom-right: {path}");
        }
    }

    #[test]
    fn ocean_wave_spans_the_full_width() {
        let path = ocean_wave_path(380.0, OCEAN_H, 5.0, 0.02, 0.0, 47.0);
        assert!(path.contains("L 380 "), "wave must reach the right edge");
        // 380 / 8px steps => 48 surface points plus the two bottom corners.
        assert!(path.matches("L ").count() >= 48);
    }

    #[test]
    fn drain_level_fills_and_empties_the_pool() {
        // Fully loaded: the waterline is at its natural level.
        assert_eq!(drain_level(47.0, 1.0), 47.0);
        // Fully unloaded: the waterline has sunk to the bottom edge.
        assert_eq!(drain_level(47.0, 0.0), OCEAN_H);
        // Draining is monotonic.
        let mid = drain_level(47.0, 0.5);
        assert!(mid > 47.0 && mid < OCEAN_H);
    }

    // ── VU dot matrix (voice_card) ───────────────────────────────────

    #[test]
    fn led_columns_stay_normalized() {
        for i in 0..20 {
            for &(rec, proc, ready) in &[(true, false, true), (true, false, false), (false, true, true)] {
                let t = led_column_target(i, 20, 1.0, 2.0, rec, proc, ready, 1.0);
                assert!((0.0..=1.0).contains(&t), "column {i} out of range: {t}");
            }
        }
    }

    #[test]
    fn led_envelope_peaks_in_the_centre() {
        let edge = led_column_target(0, 20, 0.6, 0.0, true, false, true, 0.5);
        let centre = led_column_target(10, 20, 0.6, 0.0, true, false, true, 0.5);
        assert!(centre > edge, "VU matrix must be centre-weighted");
    }

    #[test]
    fn led_ballistics_attack_fast_and_decay_slow() {
        // Attack: jumps straight to a louder target.
        assert_eq!(led_step(0.2, 0.9), 0.9);
        // Release: decays gradually instead of snapping down.
        let decayed = led_step(0.9, 0.1);
        assert!(decayed < 0.9 && decayed > 0.5);
    }

    #[test]
    fn led_quiet_speech_still_lights_the_meter() {
        // Regression for the sqrt sensitivity curve: a quiet-but-audible
        // level must light at least the bottom row of the centre column.
        let t = led_column_target(10, 20, 0.05, 0.0, true, false, true, 0.5);
        assert!(t * 6.0 >= 1.0, "quiet speech should light an LED, got {t}");
    }

    // ── Mono bars (mono_bars) ─────────────────────────────────────────

    #[test]
    fn mono_bars_stay_within_range() {
        for i in 0..5 {
            for &(rec, proc, ready) in &[(true, false, true), (true, false, false), (false, true, true), (false, false, true)] {
                for &phase in &[0.0_f32, 1.3, 5.0, 12.0] {
                    let h = mono_bar_height(i, 5, 1.0, phase, rec, proc, ready, 1.0);
                    assert!(
                        (MONO_BAR_MIN..=MONO_BAR_MAX).contains(&h),
                        "bar {i} out of range: {h}"
                    );
                }
            }
        }
    }

    #[test]
    fn mono_bars_sit_at_baseline_when_idle() {
        assert_eq!(mono_bar_height(2, 5, 1.0, 0.0, false, false, true, 1.0), MONO_BAR_MIN);
        assert_eq!(mono_bar_height(2, 5, 1.0, 0.0, true, false, false, 1.0), MONO_BAR_MIN);
    }

    #[test]
    fn mono_bars_grow_with_level() {
        let quiet = mono_bar_height(2, 5, 0.0, 0.0, true, false, true, 1.0);
        let loud = mono_bar_height(2, 5, 1.0, 0.0, true, false, true, 1.0);
        assert_eq!(quiet, MONO_BAR_MIN);
        assert!(loud > quiet, "bars must grow louder with voice level");
    }

    #[test]
    fn mono_bars_are_centre_weighted() {
        let edge = mono_bar_height(0, 5, 0.8, 0.0, true, false, true, 0.5);
        let centre = mono_bar_height(2, 5, 0.8, 0.0, true, false, true, 0.5);
        assert!(centre >= edge, "centre bar should be at least as tall as an edge bar");
    }

    // ── Neon spectrum (spectrum) ────────────────────────────────────────

    #[test]
    fn spectrum_bars_stay_within_range() {
        for i in 0..SPECTRUM_BARS {
            for &(rec, proc, ready) in &[(true, false, true), (true, false, false), (false, true, true), (false, false, true)] {
                for &phase in &[0.0_f32, 2.2, 7.7] {
                    let h = spectrum_bar_height(i, SPECTRUM_BARS, 1.0, phase, rec, proc, ready, 1.0);
                    assert!(
                        (SPECTRUM_MIN..=SPECTRUM_MAX).contains(&h),
                        "band {i} out of range: {h}"
                    );
                }
            }
        }
    }

    #[test]
    fn spectrum_bars_sit_at_floor_when_idle() {
        assert_eq!(spectrum_bar_height(0, SPECTRUM_BARS, 1.0, 0.0, false, false, true, 1.0), SPECTRUM_MIN);
    }

    #[test]
    fn spectrum_bars_react_to_level() {
        let quiet = spectrum_bar_height(0, SPECTRUM_BARS, 0.0, 0.0, true, false, true, 1.0);
        let loud = spectrum_bar_height(0, SPECTRUM_BARS, 1.0, 0.0, true, false, true, 1.0);
        assert_eq!(quiet, SPECTRUM_MIN);
        assert!(loud > quiet, "spectrum bars must rise with voice level");
    }

    // ── Retro terminal (terminal) ───────────────────────────────────────

    #[test]
    fn ascii_meter_is_always_the_configured_width() {
        for &(rec, proc, ready) in &[(true, false, true), (true, false, false), (false, true, true), (false, false, true)] {
            let m = ascii_meter(0.5, 1.0, rec, proc, ready);
            assert_eq!(m.chars().count(), ASCII_METER_WIDTH);
        }
    }

    #[test]
    fn ascii_meter_fills_with_level_when_recording() {
        assert_eq!(ascii_meter(0.0, 0.0, true, false, true), "·".repeat(ASCII_METER_WIDTH));
        assert_eq!(ascii_meter(1.0, 0.0, true, false, true), "█".repeat(ASCII_METER_WIDTH));

        let half = ascii_meter(0.5, 0.0, true, false, true);
        let filled = half.chars().filter(|&c| c == '█').count();
        assert_eq!(filled, ASCII_METER_WIDTH / 2);
    }

    #[test]
    fn ascii_meter_scans_while_processing() {
        let m = ascii_meter(0.5, 0.0, false, true, true);
        assert_eq!(m.chars().filter(|&c| c == '█').count(), 1, "exactly one lit cell while scanning");

        // The scanning position must move as phase advances.
        let m2 = ascii_meter(0.5, 0.5, false, true, true);
        assert_ne!(m, m2, "scanner must move over time");
    }

    // ── Analog VU (vinyl) ────────────────────────────────────────────────

    #[test]
    fn vu_target_angle_spans_the_dial() {
        assert_eq!(vu_target_angle(0.0), VU_ANGLE_MIN);
        assert_eq!(vu_target_angle(1.0), VU_ANGLE_MAX);
        assert!(vu_target_angle(1.0) > vu_target_angle(0.0));
    }

    #[test]
    fn vu_needle_starts_at_pivot_and_stays_on_radius() {
        for angle in [VU_ANGLE_MIN, 0.0, VU_ANGLE_MAX] {
            let path = vu_needle_path(angle);
            assert!(path.starts_with(&format!("M {VU_PIVOT_X} {VU_PIVOT_Y}")));

            let nums: Vec<f32> = path
                .split_whitespace()
                .filter_map(|t| t.parse::<f32>().ok())
                .collect();
            let (tx, ty) = (nums[2], nums[3]);
            let dist = ((tx - VU_PIVOT_X).powi(2) + (ty - VU_PIVOT_Y).powi(2)).sqrt();
            assert!((dist - VU_RADIUS).abs() < 0.2, "needle tip must sit on the dial radius, got {dist}");
        }
    }

    #[test]
    fn vu_needle_points_straight_up_at_zero_angle() {
        let path = vu_needle_path(0.0);
        let nums: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        let (tx, ty) = (nums[2], nums[3]);
        assert!((tx - VU_PIVOT_X).abs() < 0.01, "zero angle must point straight up");
        assert!(ty < VU_PIVOT_Y, "needle tip must be above the pivot");
    }

    // ── Aurora ribbon (aurora) ───────────────────────────────────────────

    #[test]
    fn aurora_path_spans_the_full_width() {
        let path = aurora_path(504.0, 0.5, 0.0, false, true);
        let nums: Vec<f32> = path.split_whitespace().filter_map(|t| t.parse::<f32>().ok()).collect();
        assert_eq!(nums[0], 0.0, "trace must start at the left edge");
        assert!(nums[nums.len() - 2] >= 498.0, "trace must reach the right edge");
    }

    #[test]
    fn aurora_swing_grows_with_level() {
        let quiet = aurora_path(120.0, 0.0, 1.3, false, true);
        let loud = aurora_path(120.0, 1.0, 1.3, false, true);
        let spread = |p: &str| -> f32 {
            let ys: Vec<f32> = p
                .split_whitespace()
                .filter_map(|t| t.parse::<f32>().ok())
                .skip(1)
                .step_by(2)
                .collect();
            ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min)
        };
        assert!(spread(&loud) > spread(&quiet), "louder voice must widen the ribbon's swing");
    }

    // ── Holo ring (holo) ─────────────────────────────────────────────────

    #[test]
    fn holo_ring_path_is_closed_and_centred() {
        let path = holo_ring_path(0.0);
        assert!(path.starts_with("M "));
        assert!(path.ends_with(" Z"));
        let nums: Vec<f32> = path
            .trim_end_matches(" Z")
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        for chunk in nums.chunks(2) {
            let (x, y) = (chunk[0], chunk[1]);
            let dist = ((x - HOLO_CX).powi(2) + (y - HOLO_CY).powi(2)).sqrt();
            assert!(dist <= HOLO_RX + 0.1, "ring point must stay within the major radius, got {dist}");
        }
    }

    #[test]
    fn holo_node_orbits_the_centre() {
        let (x, y) = holo_node_pos(0.0, 0.0);
        assert!((x - (HOLO_CX + HOLO_RX)).abs() < 0.01, "node at angle 0 sits on the rim's +x edge");
        assert!((y - HOLO_CY).abs() < 0.01);
    }

    #[test]
    fn holo_node_flashes_on_sweep_pass_and_fades_after() {
        let on_pass = holo_node_opacity(60.0, 60.0);
        let half_turn_away = holo_node_opacity(60.0, 240.0);
        assert!(on_pass > half_turn_away, "node bearing matching the sweep must be brighter");
        assert!(on_pass <= 1.0 && half_turn_away >= 0.15);
    }

    // ── Frequency petals (petals) ────────────────────────────────────────

    #[test]
    fn petal_length_grows_with_level() {
        let quiet = petal_length(0, PETAL_COUNT, 0.0, 0.0, true, false, true, 0.0);
        let loud = petal_length(0, PETAL_COUNT, 1.0, 0.0, true, false, true, 0.0);
        assert!(loud > quiet);
    }

    #[test]
    fn petal_length_sits_at_baseline_when_idle() {
        let idle = petal_length(3, PETAL_COUNT, 0.8, 1.0, false, false, true, 0.5);
        assert_eq!(idle, 14.0);
    }

    #[test]
    fn petal_spoke_path_is_anchored_at_the_centre() {
        let path = petal_spoke_path(0.0, 30.0);
        let nums: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .collect();
        let (x0, y0) = (nums[0], nums[1]);
        let dist = ((x0 - PETAL_CX).powi(2) + (y0 - PETAL_CY).powi(2)).sqrt();
        assert!(dist <= PETAL_HALF_W + 0.1, "spoke base must sit near the hub centre");
    }

    #[test]
    fn petal_group_path_only_includes_every_third_spoke() {
        let lengths = vec![20.0; PETAL_COUNT];
        let group = petal_group_path(&lengths, 0);
        // Each spoke contributes exactly one "M" subpath start.
        let count = group.matches('M').count();
        assert_eq!(count, PETAL_COUNT / 3);
    }

    // ── Minimal dot strip (dotstrip) ─────────────────────────────────────

    #[test]
    fn dotstrip_scale_grows_with_level() {
        let quiet = dotstrip_scale(4, DOTSTRIP_DOTS, 0.0, 0.0, true, false, true, 0.0);
        let loud = dotstrip_scale(4, DOTSTRIP_DOTS, 1.0, 0.0, true, false, true, 0.0);
        assert!(loud > quiet);
    }

    #[test]
    fn dotstrip_scale_sits_at_baseline_when_idle() {
        let idle = dotstrip_scale(4, DOTSTRIP_DOTS, 0.9, 1.0, false, false, true, 0.5);
        assert_eq!(idle, 1.0);
    }

    #[test]
    fn dotstrip_scale_is_centre_weighted() {
        let centre = dotstrip_scale(4, DOTSTRIP_DOTS, 1.0, 0.0, true, false, true, 0.0);
        let edge = dotstrip_scale(0, DOTSTRIP_DOTS, 1.0, 0.0, true, false, true, 0.0);
        assert!(centre >= edge, "centre dot should react at least as strongly as an edge dot");
    }

    // ── Starfield spectrum (starfield) ───────────────────────────────────

    #[test]
    fn starfield_layout_is_deterministic_and_in_bounds() {
        let mut seed_a = 99991_u32;
        let mut seed_b = 99991_u32;
        let stars_a = build_starfield(&mut seed_a);
        let stars_b = build_starfield(&mut seed_b);
        assert_eq!(stars_a.len(), STARFIELD_STAR_COUNT);
        for (a, b) in stars_a.iter().zip(stars_b.iter()) {
            assert_eq!(a.x, b.x, "same seed must produce the same layout");
            assert_eq!(a.y, b.y);
        }
        for s in &stars_a {
            assert!(s.x >= 0.0 && s.x <= STARFIELD_W);
            assert!(s.y >= 0.0 && s.y <= STARFIELD_H);
        }
    }

    #[test]
    fn star_twinkle_brightens_with_level() {
        let quiet = star_twinkle(0.2, 0.0, 0.0, 0.0);
        let loud = star_twinkle(0.2, 1.0, 0.0, 0.0);
        assert!(loud > quiet);
    }

    #[test]
    fn starfield_path_has_one_point_per_sample() {
        let hist: VecDeque<f32> = VecDeque::from(vec![0.0_f32; 126]);
        let path = starfield_path(&hist);
        assert_eq!(path.matches('L').count() + 1, hist.len());
    }

    #[test]
    fn starfield_trace_stays_inside_the_panel() {
        let mut hist: VecDeque<f32> = VecDeque::from(vec![0.0_f32; 126]);
        hist[60] = 5.0;
        let path = starfield_path(&hist);
        let ys: Vec<f32> = path
            .split_whitespace()
            .filter_map(|t| t.parse::<f32>().ok())
            .skip(1)
            .step_by(2)
            .collect();
        for y in ys {
            assert!(y >= 0.0 && y <= STARFIELD_H, "trace must stay inside the panel, got {y}");
        }
    }
}
