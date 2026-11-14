//! Graph mode on real singing: note detection, dragged notes, stretched notes, splits.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::*;
use solplugs_pitch_core::notes::{Formant, NoteEdits, analyse, render};
use solplugs_pitch_core::realtime::Range;
use solplugs_pitch_core::scale::{freq_to_midi, note_name};

const FORMANT: Formant = Formant {
    preserve: true,
    shift: 0.0,
};

/// Median detected pitch (MIDI) of `y` over source span `s..e`, middle 60%.
fn pitch_in(y: &[f32], sr: f32, s: i64, e: i64) -> f32 {
    let len = e - s;
    let (a, b) = ((s + len / 5) as usize, (e - len / 5) as usize);
    let tr = track(&y[..b.min(y.len())], sr, 128);
    let v: Vec<f32> = tr
        .iter()
        .filter(|(t, f, c)| *t >= a && *t < b && *c > 0.85 && *f > 0.0)
        .map(|(_, f, _)| freq_to_midi(*f))
        .collect();
    percentile(&v, 0.5)
}

#[test]
fn notes_are_found_on_real_singing() {
    for name in ["singing-female.wav", "vignesh.wav", "soprano-E4.wav"] {
        let (x, sr) = load(name);
        let an = analyse(&x, sr, Range::Mid);
        let e = NoteEdits::new(&an);
        let list: Vec<String> = e
            .notes
            .iter()
            .map(|n| {
                format!(
                    "{}@{:.2}s+{:.0}ms",
                    note_name(n.pitch.round() as i32),
                    n.src_start as f32 / sr,
                    (n.src_end - n.src_start) as f32 * 1000.0 / sr
                )
            })
            .collect();
        println!("{name}: {} notes: {}", e.notes.len(), list.join(" "));
        assert!(!e.notes.is_empty(), "{name}");
        for w in e.notes.windows(2) {
            assert!(w[0].src_end <= w[1].src_start, "{name}: overlap");
        }
    }
    // The soprano holds one note: one long note, E4.
    let (x, sr) = load("soprano-E4.wav");
    let e = NoteEdits::new(&analyse(&x, sr, Range::High));
    let longest = e
        .notes
        .iter()
        .max_by_key(|n| n.src_end - n.src_start)
        .unwrap();
    assert_eq!(longest.pitch.round() as i32, 64);
    assert!((longest.src_end - longest.src_start) as f32 / sr > 1.0);
}

#[test]
fn unedited_render_is_the_take() {
    for name in ["singing-female.wav", "vignesh.wav"] {
        let (x, sr) = load(name);
        let an = analyse(&x, sr, Range::Mid);
        let e = NoteEdits::new(&an);
        let y = render(&[&x], &an, &e, FORMANT).remove(0);
        let err = x
            .iter()
            .zip(&y)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        println!("{name}: max |error| {err:e}");
        assert!(err < 1e-4, "{name}: {err}");
    }
}

#[test]
fn dragging_a_wrong_note_fixes_it_cleanly() {
    let (orig, sr) = load("singing-female.wav");
    // Make a real wrong note: render the take with its long A4 sung 60 cents sharp, then
    // treat that as the recording to fix.
    let an0 = analyse(&orig, sr, Range::Mid);
    let mut e0 = NoteEdits::new(&an0);
    let a4 = e0
        .notes
        .iter()
        .position(|n| n.pitch.round() as i32 == 69)
        .expect("the take has an A4");
    e0.set_shift(a4, 0.6);
    let x = render(&[&orig], &an0, &e0, FORMANT).remove(0);
    dump("graph-sour", &x, sr);

    let an = analyse(&x, sr, Range::Mid);
    let mut e = NoteEdits::new(&an);
    let span = e0.notes[a4];
    let i = e
        .notes
        .iter()
        .position(|n| {
            n.src_start < span.src_end
                && n.src_end > span.src_start
                && (n.src_end - n.src_start) as f32 > 0.3 * sr
        })
        .expect("the sour note is found");
    let n = e.notes[i];
    let before = pitch_in(&x, sr, n.src_start, n.src_end);
    // Drag it to A4, and another note up a whole tone.
    e.set_shift(i, 69.0 - n.pitch);
    let j = (i + 2) % e.notes.len();
    let m = e.notes[j];
    e.set_shift(j, 2.0);
    let y = render(&[&x], &an, &e, FORMANT).remove(0);
    dump("graph-drag", &y, sr);
    let after = pitch_in(&y, sr, n.src_start, n.src_end);
    let moved = pitch_in(&y, sr, m.src_start, m.src_end) - pitch_in(&x, sr, m.src_start, m.src_end);
    let clicks = new_clicks(&x, &y, sr);
    println!(
        "sour note {i}: {before:.2} -> {after:.2} (A4 = 69); note {j} moved {moved:.2} st; new clicks {clicks:.1}"
    );
    assert!((before - 69.6).abs() < 0.1, "sour note sung at {before}");
    assert!((after - 69.0).abs() < 0.06, "after {after}");
    assert!((moved - 2.0).abs() < 0.1, "moved {moved}");
    assert!(clicks < 4.0, "clicks {clicks}");
    // Notes not next to an edit are untouched.
    if let Some(quiet) = e
        .notes
        .iter()
        .enumerate()
        .find(|(k, _)| k.abs_diff(i) > 1 && k.abs_diff(j) > 1)
        .map(|(_, n)| *n)
    {
        let span = quiet.src_start as usize + 2000..quiet.src_end as usize - 2000;
        let err = span.map(|t| (x[t] - y[t]).abs()).fold(0f32, f32::max);
        assert!(err < 1e-4, "untouched note changed by {err}");
    }
}

#[test]
fn stretching_and_splitting_stay_clean() {
    let (x, sr) = load("singing-female.wav");
    let an = analyse(&x, sr, Range::Mid);
    let mut e = NoteEdits::new(&an);
    // Stretch the longest note that has a contiguous follower by 30%.
    let i = (0..e.notes.len() - 1)
        .filter(|&k| e.notes[k].src_end == e.notes[k + 1].src_start)
        .max_by_key(|&k| e.notes[k].src_end - e.notes[k].src_start)
        .unwrap_or(0);
    let n = e.notes[i];
    let want = n.dst_start + (n.src_end - n.src_start) * 13 / 10;
    let got = e.set_end(i, want, sr);
    // Split another note and move its second half up a semitone.
    let k = (i + 3) % e.notes.len();
    let s = e.notes[k];
    assert!(e.split(k, (s.dst_start + s.dst_end) / 2, &an));
    e.set_shift(k + 1, 1.0);
    let y = render(&[&x], &an, &e, FORMANT).remove(0);
    dump("graph-stretch-split", &y, sr);
    assert_eq!(y.len(), x.len());
    // The stretched note keeps its pitch.
    let p_in = pitch_in(&x, sr, n.src_start, n.src_end);
    let p_out = pitch_in(&y, sr, n.dst_start, got);
    let clicks = new_clicks(&x, &y, sr);
    println!(
        "stretched note {i} to {:.0}%: pitch {p_in:.2} -> {p_out:.2}; new clicks {clicks:.1}",
        (got - n.dst_start) as f32 * 100.0 / (n.src_end - n.src_start) as f32
    );
    assert!((p_out - p_in).abs() < 0.15);
    assert!(clicks < 4.0, "clicks {clicks}");
}

#[test]
fn the_click_detector_catches_clicks() {
    let (x, sr) = load("singing-female.wav");
    let mut y = x.clone();
    // A 2-sample discontinuity in the middle of a note.
    let t = (1.5 * sr) as usize;
    y[t] += 0.15;
    y[t + 1] -= 0.1;
    let c = new_clicks(&x, &y, sr);
    println!("planted click scores {c:.1}");
    assert!(c > 5.0);
}
