# Attribution

Every non-code asset in this repository, where it came from, and its licence.

| Asset | Source | Licence |
|---|---|---|
| `crates/pitch-core/testdata/singing-female.wav` | MTG sms-tools teaching sounds, [github.com/MTG/sms-tools-materials](https://github.com/MTG/sms-tools-materials) `sounds/singing-female.wav` (Music Technology Group, Universitat Pompeu Fabra) | CC BY 4.0 |
| `crates/pitch-core/testdata/soprano-E4.wav` | Same collection, `sounds/soprano-E4.wav` | CC BY 4.0 |
| `crates/pitch-core/testdata/vignesh.wav` | Same collection, `sounds/vignesh.wav` (male Carnatic singing) | CC BY 4.0 |

The test vocals are used unmodified, only by the test suite; they are not shipped in the plugin.

The plugin's UI uses egui's bundled default fonts (`egui` feature `default_fonts`): Ubuntu-Light (Ubuntu Font Licence 1.0), Hack (MIT/Bitstream Vera), Noto Emoji (OFL 1.1), egui-icons and emoji-icon-font (MIT). They ship inside the egui crates under their own licences.
