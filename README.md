# SolPlugs
SolPlugs — the SolPlugs plugin bundle for Solara: reference analyzer, EQ, compressor and more as native VST3/CLAP plugins, usable in any DAW.

## SolPlugs Reference

A reference track analyzer: A/B against references with loudness matching, DIFF (where the mix falls outside the references' range), PARTS (sub, kick and snare outlines), meters (LUFS, true peak, RMS, width, correlation, tonal balance, dynamics), a library of up to 60 references with groups, Deezer preview search (off by default; previews are streamed, only the analysis is kept) and SolRoute feed-in (any audio playing on the machine). Zero latency; the mix passes through untouched.

| Crate | What it is |
|---|---|
| `crates/ref-core` | Analysis: loudness, true peak, stereo, tonal balance, PARTS, group spans, DIFF, A/B switch |
| `crates/ref-io` | Decoding (WAV, FLAC, MP3), the reference library, Deezer search |
| `crates/route` | SolRoute: system audio capture (WASAPI loopback on Windows) |
| `crates/ref-app` | The embeddable app: audio-thread processor, background worker, egui UI. Solara embeds this. |
| `crates/plugin-reference` | VST3 and CLAP wrappers and the plugin window |

## SolPlugs Samples

A sample manager: every sample is a card with artwork, waveform, BPM, key and length in its genre colours. Import a folder (samples stay where they are), tag, filter by genre, BPM, key and rating, preview with a click, and drag a sample onto a track or into any DAW. Build your own packs with covers, descriptions and tags, and export them as a folder. Built-in tools (trim, trim silence, fades, reverse, normalize, save a time selection Pre FX, Post FX or both) save edited versions that stay linked to their raw. A live spectrum runs while a sample previews. Works as a VST3/CLAP plugin and in its own window.

| Crate | What it is |
|---|---|
| `crates/songsense` | Binding to Song Sense's BPM/key detector (see below) |
| `crates/samples-core` | The library (index file, import, tags, packs, versions), decoding (WAV, AIFF, FLAC, MP3), tools, export |
| `crates/samples-app` | The embeddable app: engine, preview player, egui UI. Solara embeds this. |
| `crates/plugin-samples` | VST3 and CLAP wrappers and the plugin window |
| `crates/samples-standalone` | The standalone window (`solplugs-samples`) |

The library is a data file, `%APPDATA%\SolPlugs\Samples\library.json`, with edited versions in `edits\` and artwork thumbnails in `art\` beside it. Builds and reinstalls never touch it.

**BPM and key** come from a file's tags, then its name ("Loop 140bpm Fm.wav"), then Song Sense's detector. The detector is Song Sense's own C++ core, compiled at build time from a private checkout of `AI-Vault` (`SONGSENSE_CORE_DIR`, or an `AI-Vault` folder next to this repo). None of its source is in SolPlugs. Without the checkout everything still builds, and BPM/key detection is simply off.

## Building

```
cargo test --workspace
cargo xtask bundle [reference|samples]   # dist/<Name>.vst3 and .clap, one binary per format
cargo xtask install [reference|samples]  # per-user VST3 and CLAP folders
cargo xtask probe-vst3 <binary>          # a tiny VST3 host that walks a DAW's scan and process calls
cargo run -p solplugs-samples-standalone --release   # SolPlugs Samples in its own window
```

Live checks that need network or a sound device are `#[ignore]`d: `cargo test -- --ignored`.
