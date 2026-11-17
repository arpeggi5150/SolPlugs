# SolPlugs
SolPlugs — the SolPlugs plugin bundle for Solara: reference analyzer, EQ, compressor and more as native VST3/CLAP plugins, usable in any DAW.

## SolPlugs Pitch

Monophonic pitch correction: an automatic mode for live audio and a graph mode for note-by-note editing.

- **Automatic:** key and scale (chromatic, major, minor and more, or a custom note set), retune speed (0 ms is a hard snap), humanize, amount, vibrato amount, formant preservation (on by default) and formant shift.
- **Graph:** press Capture and play the part. The take shows up as note blobs. Drag a note to change its pitch (Shift for fine), drag an edge to stretch it, double-click to split. Undo/redo, snap to scale, reset.
- **Detection and shifting:** McLeod pitch method and TD-PSOLA, both written from scratch in Rust (`crates/pitch-core`).
- **Latency:** set by the Range picker. High (100-1400 Hz) is 25.3 ms, Mid (75-1000 Hz) is 32.0 ms, Low (55-700 Hz) is 41.7 ms, at any sample rate. The plugin reports it to the host. Bypass is bit-identical to the input, delayed by the same latency.

Build and install: `cargo xtask bundle` then `cargo xtask install`. Check a vocal take with `cargo run --release -p solplugs-pitch-app --example vocal_report -- take.wav out_dir`.

## Releases

Downloads are published to [arpeggi5150/SolPlugs-Releases](https://github.com/arpeggi5150/SolPlugs-Releases). Pushing a version tag that matches `[workspace.package] version` (for example `v0.1.0`), or running *Actions → Publish* by hand, runs `.github/workflows/publish.yml`: it bundles the plugins on a GitHub-hosted Windows runner and publishes one zip per format (VST3, CLAP) plus `SHA256SUMS.txt`. It reads the upload token from the repository secret `SOLPLUGS_RELEASES_TOKEN` (Contents read + write on SolPlugs-Releases only) and stops with a plain error, before building, if that secret is missing, the tag doesn't match the version, or the release already exists.
