//! End-to-end check on real music: three Deezer previews become a reference group, a
//! fourth stands in for the mix, and DIFF and PARTS run on them. Needs network access:
//! `cargo test -p solplugs-ref-io --release -- --ignored --nocapture live_references`.
#![allow(clippy::expect_used, clippy::print_stdout)]

use solplugs_ref_core::diff::{DiffOptions, diff};
use solplugs_ref_core::group::group_stats;
use solplugs_ref_core::profile::{Profile, analyze};
use solplugs_ref_io::deezer;

fn preview_profile(query: &str) -> Profile {
    let hits = deezer::search(query, 3).expect("search");
    let t = hits.first().expect("no results");
    let audio = deezer::fetch_preview(t).expect("preview");
    let p = analyze(&audio.left, &audio.right, audio.sample_rate);
    println!(
        "{} - {}: {:.1} s, {:.1} LUFS",
        t.artist,
        t.title,
        audio.seconds(),
        p.integrated_lufs.unwrap_or(f64::NAN)
    );
    for part in &p.parts.elements {
        println!(
            "    {:?}: found={} level={:.1} dB {:.0}-{:.0} Hz peak {:.0} Hz, {:.0} hits/min",
            part.kind,
            part.found,
            part.level_db,
            part.lo_hz,
            part.hi_hz,
            part.peak_hz,
            part.hits_per_min
        );
    }
    p
}

#[test]
#[ignore = "needs network access to api.deezer.com"]
fn live_references_group_diff_and_parts() {
    let refs: Vec<Profile> = [
        "daft punk around the world",
        "dua lipa levitating",
        "the weeknd blinding lights",
    ]
    .iter()
    .map(|q| preview_profile(q))
    .collect();
    let mix = preview_profile("billie eilish bad guy");

    let group = group_stats(&refs.iter().collect::<Vec<_>>()).expect("group");
    let d = diff(&mix, &group, DiffOptions::default());
    let outside = d.bands.iter().filter(|b| b.outside.abs() > 0.0).count();
    println!(
        "DIFF: {outside} of {} bands outside the group range",
        d.bands.len()
    );
    for i in &d.issues {
        println!("  issue: {}", i.text);
    }
    for m in &d.metrics {
        println!("  metric: {m:?}");
    }

    // Every reference should show at least a kick: these are all four-on-the-floor or
    // straight pop beats.
    for p in &refs {
        assert!(
            p.parts.elements.iter().any(|e| e.found),
            "no parts found in a reference"
        );
    }
    assert_eq!(d.bands.len(), solplugs_ref_core::bands::BANDS);
}
