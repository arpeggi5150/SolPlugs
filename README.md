# SolPlugs
SolPlugs — the SolPlugs plugin bundle for Solara: reference analyzer, EQ, compressor and more as native VST3/CLAP plugins, usable in any DAW.

## MIDI Keys (`crates/midi-keys`, `crates/midi-keys-app`)
Play synths from hardware MIDI keyboards: 1 to 4 tracks, each with its own MIDI input (and channel), its own synth (any VST3 instrument, hosted by Solara's VST3 host, or Solara's built-in synths), volume and mute. Low latency: a small audio buffer (128 frames by default) on the Windows audio output.

- `solplugs-midi-keys` is the shared piece: MIDI parsing and routing, the lock-free mixer, the synth list. Solara can use it for its own hardware MIDI input.
- `solplug-midi-keys` is the program Command Center's MIDI module runs (JSON lines on stdin/stdout; see `crates/midi-keys-app/src/main.rs`).

Build: `cargo build --release -p solplug-midi-keys`. Test: `cargo test`.
