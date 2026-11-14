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

```
cargo test --workspace
cargo xtask bundle        # dist/SolPlugs Reference.vst3 and .clap, one binary per format
cargo xtask install       # per-user VST3 and CLAP folders
cargo xtask probe-vst3 <binary>   # a tiny VST3 host that walks a DAW's scan and process calls
```

Live checks that need network or a sound device are `#[ignore]`d: `cargo test -- --ignored`.
