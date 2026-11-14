# SolPlugs
SolPlugs — the SolPlugs plugin bundle for Solara: reference analyzer, EQ, compressor and more as native VST3/CLAP plugins, usable in any DAW.

## SolPlugs Pitch

Monophonic pitch correction: an automatic mode for live audio and a graph mode for note-by-note editing.

- **Automatic:** key and scale (chromatic, major, minor and more, or a custom note set), retune speed (0 ms is a hard snap), humanize, amount, vibrato amount, formant preservation (on by default) and formant shift.
- **Graph:** press Capture and play the part. The take shows up as note blobs. Drag a note to change its pitch (Shift for fine), drag an edge to stretch it, double-click to split. Undo/redo, snap to scale, reset.
- **Detection and shifting:** McLeod pitch method and TD-PSOLA, both written from scratch in Rust (`crates/pitch-core`).
- **Latency:** set by the Range picker. High (100-1400 Hz) is 25.3 ms, Mid (75-1000 Hz) is 32.0 ms, Low (55-700 Hz) is 41.7 ms, at any sample rate. The plugin reports it to the host. Bypass is bit-identical to the input, delayed by the same latency.

Build and install: `cargo xtask bundle` then `cargo xtask install`. Check a vocal take with `cargo run --release -p solplugs-pitch-app --example vocal_report -- take.wav out_dir`.

## SolPlugs Compressor

One compressor with five circuits in it. Pick a mode, dial it in, hear the character change. Each mode remembers its own settings, so you can flip between them and compare.

| Mode | Character | Styled after | Latency |
|---|---|---|---|
| **VCA** | Fast, punchy, clean glue | 80s console bus compressors | 28 samples (0.58 ms at 48 kHz) |
| **Opto** | Smooth, slow, never grabs | 60s optical tube levellers | 28 samples |
| **FET** | Fast, aggressive, gritty; ALL = all buttons in | 70s FET limiters | 28 samples |
| **Vari-Mu** | Warm tube glue, program-dependent release | 50s variable-mu tube limiters | 28 samples |
| **Clean** | Transparent, no colour, optional lookahead | No hardware: modern digital | 0, plus the lookahead (up to 10 ms) |

- **Controls on every mode:** threshold, ratio, attack, release, knee, Gain In, Gain Out (the makeup), Mix, Wet and Dry levels, peak/RMS detector, auto release, sidechain high-pass, harmonic colour (not in Clean).
- **MATCH and auto-makeup:** MATCH sets Gain Out once so the average peak after matches the average peak before. Auto-makeup tracks it live; turning it off keeps the gain it reached.
- **Stereo:** linked (default) or dual mono. External sidechain on inputs 3/4 (Ext SC in the footer).
- **Bypass** is bit-identical to the input, delayed by the latency. A/B holds two complete setups.
- **DSP:** written from scratch in Rust (`crates/comp-core`). The coloured modes run their saturation 4x oversampled through half-band filters, so nothing aliases.
- **Presets:** one starting point per mode, plus Vocals, Drums, Bass, Mix bus, Mastering and Parallel smash.

Build and install: `cargo xtask install compressor`. Run it on its own (live input or a dropped WAV): `cargo run --release -p solplugs-comp-standalone`. Solara embeds the editor and engine from `crates/comp-app`.
