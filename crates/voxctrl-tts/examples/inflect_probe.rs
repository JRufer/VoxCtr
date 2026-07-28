//! Standalone probe for the Inflect-Micro-v2 pipeline.
//!
//! Runs phonemization → tokenization → both ONNX graphs and writes a WAV,
//! with no Tauri, no event plumbing and no audio device involved. Each stage
//! prints before and after it runs, so if the pipeline stalls or dies the last
//! line printed names the stage responsible — which the app cannot tell you,
//! because a hang there could equally be synthesis or playback.
//!
//! ```bash
//! cargo run -p voxctrl-tts --features inflect-micro --example inflect_probe
//! cargo run -p voxctrl-tts --features inflect-micro --example inflect_probe -- "custom text"
//! ```

#[cfg(not(feature = "inflect-micro"))]
fn main() {
    eprintln!(
        "This probe needs the `inflect-micro` feature:\n  \
         cargo run -p voxctrl-tts --features inflect-micro --example inflect_probe"
    );
    std::process::exit(1);
}

#[cfg(feature = "inflect-micro")]
fn main() {
    use std::io::Write;
    use voxctrl_tts::inflect::{self, model::InflectModel, phonemes};

    // Unbuffered so a stall leaves the last completed stage on screen.
    macro_rules! step {
        ($($arg:tt)*) => {{
            print!($($arg)*);
            let _ = std::io::stdout().flush();
        }};
    }

    let text = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Hi this is Inflect Micro speaking from VoxCtrl".to_string());

    let dir = inflect::resolve_model_dir("");
    println!("model dir : {}", dir.display());
    println!("text      : {text:?}\n");

    // ── 1. eSpeak-NG frontend ────────────────────────────────────────────────
    step!("[1/6] espeak-ng present ... ");
    if !phonemes::espeak_available() {
        println!("NO");
        eprintln!("\nespeak-ng is not on PATH. Install it and re-run.");
        std::process::exit(1);
    }
    println!("yes");

    step!("[2/6] phonemize ... ");
    let ipa = match phonemes::phonemize(&text) {
        Ok(v) => v,
        Err(e) => {
            println!("FAILED");
            eprintln!("\n{e:#}");
            std::process::exit(1);
        }
    };
    println!("ok\n        IPA: {ipa}");

    // ── 2. Symbol table ──────────────────────────────────────────────────────
    step!("[3/6] load symbol table ... ");
    let vocab = match phonemes::PhonemeVocab::load(&dir) {
        Ok(Some(v)) => v,
        Ok(None) => {
            println!("NOT FOUND");
            eprintln!(
                "\nNo file in {} parses as a symbol table. Put the export's \
                 text/symbols.py there.",
                dir.display()
            );
            std::process::exit(1);
        }
        Err(e) => {
            println!("FAILED");
            eprintln!("\n{e:#}");
            std::process::exit(1);
        }
    };
    println!(
        "ok\n        {} symbols from {}",
        vocab.len(),
        vocab.source.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
    );

    let encoded = vocab.encode(&ipa);
    println!(
        "        {} tokens (expected {}), skipped: {:?}",
        encoded.ids.len(),
        ipa.chars().count() * 2 + 1,
        encoded.skipped
    );

    // ── 3. ONNX graphs ───────────────────────────────────────────────────────
    step!("[4/6] load ONNX graphs ... ");
    let mut model = match InflectModel::load(&dir) {
        Ok(m) => m,
        Err(e) => {
            println!("FAILED");
            eprintln!("\n{e:#}");
            std::process::exit(1);
        }
    };
    println!("ok");
    let sig = model.signature();
    println!("        {}:", sig.duration.file);
    for d in &sig.duration.input_details {
        println!("          in  {d}");
    }
    for d in &sig.duration.output_details {
        println!("          out {d}");
    }
    println!("        {}:", sig.decode.file);
    for d in &sig.decode.input_details {
        println!("          in  {d}");
    }
    for d in &sig.decode.output_details {
        println!("          out {d}");
    }

    // ── 4. Synthesis ─────────────────────────────────────────────────────────
    // This is the step that has never run anywhere; if the pipeline dies, it
    // almost certainly dies here.
    step!("[5/6] synthesize (this is the untested step) ... ");
    let cfg = voxctrl_config::InflectMicroConfig::default();
    let audio = match model.synthesize(&text, &cfg, 1.0, cfg.seed) {
        Ok(a) => a,
        Err(e) => {
            println!("FAILED");
            eprintln!("\n{e:#}");
            std::process::exit(1);
        }
    };
    println!("ok");

    if audio.is_empty() {
        eprintln!("\nSynthesis returned no samples — nothing to write.");
        std::process::exit(1);
    }
    let peak = audio.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let rms = (audio.iter().map(|s| (s * s) as f64).sum::<f64>() / audio.len() as f64).sqrt();
    println!(
        "        {} samples ({:.2}s at {} Hz), peak {peak:.4}, rms {rms:.4}",
        audio.len(),
        audio.len() as f32 / inflect::SAMPLE_RATE as f32,
        inflect::SAMPLE_RATE
    );
    if peak < 1e-6 {
        println!("        WARNING: output is silent — the graphs ran but produced no signal.");
    }

    // ── 5. WAV out ───────────────────────────────────────────────────────────
    // Written directly rather than played, so this says whether synthesis works
    // independently of whether audio playback does.
    step!("[6/6] write WAV ... ");
    let out = std::env::temp_dir().join("inflect_probe.wav");
    match write_wav(&out, &audio, inflect::SAMPLE_RATE) {
        Ok(()) => println!("ok\n\nWrote {}", out.display()),
        Err(e) => {
            println!("FAILED");
            eprintln!("\n{e}");
            std::process::exit(1);
        }
    }
    println!("Play it with:  aplay {}", out.display());
}

/// Minimal 16-bit PCM mono WAV writer, so the probe needs no audio crate.
#[cfg(feature = "inflect-micro")]
fn write_wav(path: &std::path::Path, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    use std::io::Write;

    let data_len = (samples.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);

    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?; // PCM header size
    f.write_all(&1u16.to_le_bytes())?; // format: PCM
    f.write_all(&1u16.to_le_bytes())?; // channels: mono
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&(sample_rate * 2).to_le_bytes())?; // byte rate
    f.write_all(&2u16.to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits per sample
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;

    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        f.write_all(&v.to_le_bytes())?;
    }
    f.flush()
}
