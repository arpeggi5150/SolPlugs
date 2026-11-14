# Attribution

Every non-code asset in this repository, where it came from, and its licence.

| Asset | Source | Licence |
|---|---|---|
| `crates/ref-io/testdata/sine-1k-1s.mp3` | Generated for this repo: `ffmpeg -f lavfi -i "sine=frequency=1000:sample_rate=44100:duration=1" -ac 2 -c:a libmp3lame -b:a 96k` | Our own output, same licence as the repo (MIT OR Apache-2.0) |

The plugin's UI uses egui's bundled default fonts (`egui` feature `default_fonts`): Ubuntu-Light (Ubuntu Font Licence 1.0), Hack (MIT/Bitstream Vera), Noto Emoji (OFL 1.1), egui-icons and emoji-icon-font (MIT). They ship inside the egui crates under their own licences.
