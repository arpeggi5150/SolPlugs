//! End to end on real files: import a folder (odd files included), tag and filter, build a pack
//! with a raw and its edits, save selections Pre/Post/Both, and export the pack.

use std::path::Path;

use solplugs_samples_core::audio::{self, Audio, WavDepth};
use solplugs_samples_core::edits::{self, Edit, Render};
use solplugs_samples_core::filter::{Query, Versions};
use solplugs_samples_core::fx::Fx;
use solplugs_samples_core::model::{Library, Pack, Source, Version};
use solplugs_samples_core::scan;
use solplugs_samples_core::store::Store;
use solplugs_samples_core::tools::{Range, Tool};

type R = Result<(), Box<dyn std::error::Error>>;

fn groove(sr: f64, secs: f64, bpm: f64) -> Audio {
    let beat = 60.0 / bpm;
    let n = (sr * secs) as usize;
    let v = (0..n)
        .map(|i| {
            let t = i as f64 / sr;
            let b = t % beat;
            let kick = 0.6 * (-b * 30.0).exp() * (2.0 * std::f64::consts::PI * 60.0 * t).sin();
            let chord: f64 = [220.0, 261.63, 329.63]
                .iter()
                .map(|f| (2.0 * std::f64::consts::PI * f * t).sin())
                .sum();
            (kick + 0.15 * chord) as f32
        })
        .collect::<Vec<f32>>();
    Audio::new(sr, vec![v.clone(), v])
}

fn one_shot(sr: f64) -> Audio {
    let n = (sr * 0.4) as usize;
    let v = (0..n)
        .map(|i| {
            let t = i as f64 / sr;
            ((-t * 12.0).exp() * (2.0 * std::f64::consts::PI * 55.0 * t).sin() * 0.8) as f32
        })
        .collect();
    Audio::new(sr, vec![v])
}

fn setup(dir: &Path) -> R {
    let trap = dir.join("Trap Kit");
    std::fs::create_dir_all(trap.join("Loops"))?;
    std::fs::create_dir_all(dir.join("House"))?;
    audio::write_wav(
        &trap.join("Loops/Dark_Loop_140bpm_Fm.wav"),
        &groove(44_100.0, 4.0, 140.0),
        WavDepth::Int24,
    )?;
    audio::write_wav(
        &trap.join("808_Long.wav"),
        &one_shot(44_100.0),
        WavDepth::Int16,
    )?;
    audio::write_wav(
        &dir.join("House/Groove.wav"),
        &groove(48_000.0, 8.0, 124.0),
        WavDepth::Float32,
    )?;
    std::fs::write(dir.join("House/broken.wav"), b"RIFF\x10\0\0\0WAVEfmt junk")?;
    std::fs::write(dir.join("House/empty.flac"), b"")?;
    std::fs::write(dir.join("notes.txt"), b"not audio")?;
    // A small AIFF one-shot.
    let pcm: Vec<i16> = (0..4410)
        .map(|i| ((i as f64 * 0.05).sin() * 12_000.0) as i16)
        .collect();
    std::fs::write(dir.join("House/Clap.aif"), aiff16(44_100, &pcm))?;
    Ok(())
}

/// 16-bit mono AIFF writer for the test.
fn aiff16(rate: u32, pcm: &[i16]) -> Vec<u8> {
    let shift = rate.leading_zeros();
    let mant = u64::from(rate) << (32 + shift);
    let exp = 16_383 + 31 - shift;
    let mut comm = Vec::new();
    comm.extend_from_slice(&1u16.to_be_bytes());
    comm.extend_from_slice(&(pcm.len() as u32).to_be_bytes());
    comm.extend_from_slice(&16u16.to_be_bytes());
    comm.extend_from_slice(&[(exp >> 8) as u8, exp as u8]);
    comm.extend_from_slice(&mant.to_be_bytes());
    let mut snd = vec![0u8; 8];
    for s in pcm {
        snd.extend_from_slice(&s.to_be_bytes());
    }
    let mut f = b"FORM\0\0\0\0AIFF".to_vec();
    for (id, body) in [(b"COMM", comm), (b"SSND", snd)] {
        f.extend_from_slice(id);
        f.extend_from_slice(&(body.len() as u32).to_be_bytes());
        f.extend_from_slice(&body);
    }
    let size = (f.len() - 8) as u32;
    f[4..8].copy_from_slice(&size.to_be_bytes());
    f
}

// One scripted walk through the brief's tests; splitting it would only pass the library around.
#[allow(clippy::cognitive_complexity)]
#[test]
fn import_tag_filter_pack_edit_export() -> R {
    let tmp = tempfile::tempdir()?;
    let samples = tmp.path().join("samples");
    setup(&samples)?;
    let store = Store::at(tmp.path().join("lib"));
    let mut lib = Library::default();

    // 1. Import: every real file in, odd files reported, nothing crashes.
    let paths = scan::collect(&samples);
    assert_eq!(paths.len(), 6, "{paths:?}");
    let report = scan::import(&mut lib, &store, &paths, |_, _, _| true);
    assert_eq!(report.added.len(), 4, "{report:?}");
    assert_eq!(report.failed.len(), 2, "{report:?}");
    let again = scan::import(&mut lib, &store, &paths, |_, _, _| true);
    assert_eq!((again.added.len(), again.known), (0, 4));

    let find = |lib: &Library, name: &str| lib.samples.iter().find(|s| s.name == name).cloned();
    let dark = find(&lib, "Dark_Loop_140bpm_Fm").ok_or("dark")?;
    assert_eq!((dark.bpm, dark.bpm_source), (Some(140.0), Source::Name));
    assert_eq!(dark.key.as_deref(), Some("F minor"));
    assert_eq!(dark.genres, vec!["Trap".to_string()]);
    assert_eq!(
        (dark.sample_rate, dark.bit_depth, dark.channels),
        (44_100, Some(24), 2)
    );
    let groove = find(&lib, "Groove").ok_or("groove")?;
    assert_eq!(groove.genres, vec!["House".to_string()]);
    if solplugs_songsense::available() {
        let b = groove.bpm.ok_or("no bpm detected")?;
        assert!((b - 124.0).abs() < 1.5, "detected {b}");
        assert_eq!(groove.bpm_source, Source::Detected);
        assert!(groove.key.is_some());
    }
    let clap = find(&lib, "Clap").ok_or("clap")?;
    assert!((clap.duration_sec - 0.1).abs() < 1e-3);
    assert_eq!(clap.bpm, None, "one-shots get no tempo");
    assert!(!dark.peaks.is_empty());

    // 2. Tag a few, filter by genre + BPM.
    for s in &mut lib.samples {
        if s.name.contains("Loop") || s.name == "808_Long" {
            s.tags.push("dark".into());
        }
    }
    let q = Query {
        genres: vec!["trap".into()],
        bpm_min: Some(130.0),
        bpm_max: Some(150.0),
        ..Query::default()
    };
    let hits: Vec<_> = q
        .run(&lib)
        .into_iter()
        .map(|i| lib.samples[i].name.clone())
        .collect();
    assert_eq!(hits, vec!["Dark_Loop_140bpm_Fm".to_string()]);
    let q = Query {
        tags: vec!["dark".into()],
        ..Query::default()
    };
    assert_eq!(q.run(&lib).len(), 2);
    let q = Query {
        key: Some("Fm".into()),
        ..Query::default()
    };
    assert_eq!(q.run(&lib).len(), 1);

    // 3/9. Tools save edited versions linked to the raw.
    let decoded = audio::decode_file(&dark.path)?;
    let all = Range::all(&decoded);
    let mut made = Vec::new();
    for tool in [
        Tool::Trim(Range::seconds(&decoded, 0.5, 1.5)),
        Tool::TrimSilence,
        Tool::FadeIn {
            range: all,
            len: 4410,
        },
        Tool::FadeOut {
            range: all,
            len: 4410,
        },
        Tool::Reverse(all),
        Tool::Normalize(all),
    ] {
        made.extend(edits::save(
            &mut lib,
            &store,
            &dark.id,
            &decoded,
            Edit::Tool(tool),
        )?);
    }
    assert_eq!(made.len(), 6);
    for id in &made {
        let e = lib.sample(id).ok_or("edit")?;
        assert!(
            matches!(&e.version, Version::Edited { parent, root, .. } if *parent == dark.id && *root == dark.id)
        );
        assert!(e.path.starts_with(store.edits()) && e.path.is_file());
    }
    let trimmed = audio::decode_file(&lib.sample(&made[0]).ok_or("t")?.path)?;
    assert!((trimmed.seconds() - 1.0).abs() < 1e-3);
    let rev = audio::decode_file(&lib.sample(&made[4]).ok_or("r")?.path)?;
    assert!((rev.channels[0][0] - decoded.channels[0][decoded.frames() - 1]).abs() < 1e-5);
    let norm = audio::decode_file(&lib.sample(&made[5]).ok_or("n")?.path)?;
    assert!((20.0 * norm.peak().log10() + 1.0).abs() < 0.01);

    // 8. Save from selection: Pre FX is dry, Post FX has the processing, Both makes two.
    lib.sample_mut(&dark.id).ok_or("d")?.fx = Fx {
        gain_db: -6.0,
        low_cut_hz: 200.0,
        ..Fx::default()
    };
    let sel = Range::seconds(&decoded, 1.0, 2.0);
    let pre = edits::save(
        &mut lib,
        &store,
        &dark.id,
        &decoded,
        Edit::Selection {
            range: sel,
            render: Render::PreFx,
        },
    )?;
    let pre_audio = audio::decode_file(&lib.sample(&pre[0]).ok_or("p")?.path)?;
    let dry = decoded.slice(sel.start, sel.end);
    assert_eq!(pre_audio.frames(), dry.frames());
    assert!(
        pre_audio.channels[0]
            .iter()
            .zip(&dry.channels[0])
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
    let post = edits::save(
        &mut lib,
        &store,
        &dark.id,
        &decoded,
        Edit::Selection {
            range: sel,
            render: Render::PostFx,
        },
    )?;
    let post_s = lib.sample(&post[0]).ok_or("q")?.clone();
    let post_audio = audio::decode_file(&post_s.path)?;
    assert!(
        post_audio.peak() < dry.peak() * 0.6,
        "post FX should be quieter"
    );
    assert!(post_s.fx.is_neutral());
    let both = edits::save(
        &mut lib,
        &store,
        &dark.id,
        &decoded,
        Edit::Selection {
            range: sel,
            render: Render::Both,
        },
    )?;
    assert_eq!(both.len(), 2);
    for id in both.iter().chain(&pre).chain(&post) {
        assert!(
            matches!(&lib.sample(id).ok_or("b")?.version, Version::Edited { parent, .. } if *parent == dark.id)
        );
    }

    // 3. A pack with a cover, holding the raw and an edit.
    let cover_src = tmp.path().join("cover.png");
    image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 40, 90, 255])).save(&cover_src)?;
    let cover = solplugs_samples_core::art::thumb_from_file(&cover_src, &store.art(), "pack-1")
        .ok_or("cover")?;
    let pid = lib.new_id();
    lib.packs.push(Pack {
        id: pid.clone(),
        name: "Night/Drive: Vol 1".into(),
        description: "mine".into(),
        cover: Some(cover),
        tags: vec!["dark".into()],
        genres: vec!["Trap".into()],
        members: vec![made[0].clone(), dark.id.clone()],
        created_unix: 0,
    });
    let q = Query {
        pack: Some(pid.clone()),
        versions: Versions::Edited,
        ..Query::default()
    };
    assert_eq!(q.run(&lib).len(), 1);

    // Library survives a save/load.
    store.save(&lib)?;
    assert_eq!(store.load()?, lib);

    // 6. Export.
    let out = solplugs_samples_core::export::export(&lib, &pid, &tmp.path().join("exports"))?;
    assert_eq!(out.copied, 2);
    assert!(out.folder.join("cover.png").is_file());
    assert!(out.folder.join("Raw/Dark_Loop_140bpm_Fm.wav").is_file());
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.folder.join("pack.json"))?)?;
    let edited = json["samples"]
        .as_array()
        .ok_or("arr")?
        .iter()
        .find(|s| s["version"] == "edited")
        .ok_or("ed")?;
    assert_eq!(edited["parent_file"], "Raw/Dark_Loop_140bpm_Fm.wav");
    assert_eq!(edited["raw_name"], "Dark_Loop_140bpm_Fm");
    assert!(
        out.folder
            .join(edited["file"].as_str().ok_or("f")?)
            .is_file()
    );

    // Removing the raw keeps its edits and their origin.
    lib.remove_sample(&dark.id);
    assert!(lib.sample(&made[0]).is_some());
    Ok(())
}
