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

## SolPlugs EQ

One EQ with eight styles in it: seven hardware types and Clean, a precise digital EQ. Each style remembers its own settings, so you can flip between them and compare.

| Style | Character | Styled after | Latency |
|---|---|---|---|
| **Pultec** | Smooth, wide tube shelves; boost and cut the same low at once | 1950s passive valve program EQs | 28 samples (0.58 ms at 48 kHz) |
| **API** | Punchy, forward; the more you boost, the narrower the band | 1970s discrete 3-band console EQs | 28 samples |
| **SSL** | Clean, precise, surgical 4-band with filters | 1980s British console channel EQs | 28 samples |
| **Neve** | Warm, big, round, transformer colour | 1970s Class A console EQs | 28 samples |
| **Clean** | Transparent, up to 24 bands, any filter type | No hardware: modern digital | 0 (zero latency) or 4608 samples (linear phase, 96 ms at 48 kHz) |
| **Massive** | Huge, smooth 4-band passive mastering curves | 1990s passive valve mastering EQs | 28 samples |
| **Maag** | Open top end; the Air band lifts without harshness | Air-band EQs | 28 samples |
| **Curve** | Bold, characterful valve console tone | Valve broadcast console EQs | 28 samples |

- **Every style:** Gain In, Gain Out (the makeup), Mix, Wet and Dry, auto-makeup, MATCH, harmonic colour (hardware styles), stereo link (default) or dual mono, bit-identical bypass, A/B.
- **Hardware styles:** the faceplate on the left; on the right a before/after spectrum analyzer with the EQ curve, before/after waveforms, a post meter and MATCH. The colour stages run 4x oversampled, so pushing them doesn't alias.
- **Clean:** up to 24 bands. Bell, shelves, cuts (6 to 48 dB/oct), notch, band-pass and tilt. Per-band solo, dynamic EQ (threshold, range, attack, release), stereo/mid/side per band. Click a peak on the analyzer to put a band there. Zero latency (matched-response IIR) or linear phase (FIR, 4608 samples at 44.1/48 kHz, 9216 at 88.2/96 kHz, 17408 at 192 kHz). The big graph has a log frequency axis with octave lines and labels, 3 dB grid lines and each band's own curve under the total; the side column has the spectrometer (overlay, side by side or stacked), big BEFORE/AFTER meters and MATCH.
- **Presets:** one per style, plus Vocals, Drums, Bass, Mix bus, Mastering, Air / sparkle and Low-end clean-up.
- **DSP:** written from scratch in Rust (`crates/eq-core`): zero-delay-feedback state-variable filters with exact responses, the compressor's oversampler and saturation, and a partitioned FFT convolver for linear phase. The plugin reports its latency to the host and updates it when the style or phase mode changes.

Build and install: `cargo xtask install eq`. Run it on its own (live input or a dropped WAV): `cargo run --release -p solplugs-eq-standalone`. Solara embeds the editor and engine from `crates/eq-app`.
