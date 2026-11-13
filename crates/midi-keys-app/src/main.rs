//! `solplug-midi-keys`: SolPlugs MIDI Keys as a program, driven by Command Center's MIDI module.
//!
//! JSON lines. Each line on stdin is a request; each answer is one line on stdout:
//! - `{"op":"list"}` → `{"ok":true,"inputs":[...],"output":"...","synths":[{id,name,vendor,kind}]}`
//! - `{"op":"apply","tracks":[{input,channel,synth,volume,mute}, ...],"buffer":128}` → the tracks' states
//! - `{"op":"note","track":0,"note":60,"velocity":100,"on":true}` (the module's test keys)
//! - `{"op":"panic"}` all notes off; `{"op":"stop"}` ends the program (closing stdin does too).
//!
//! Notes played on a keyboard are reported as `{"ev":"note","track":0,"note":60,"on":true}`.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use serde_json::{Value, json};
use std::io::{BufRead, Write};

fn say(v: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn synth_list() -> Value {
    json!(solplugs_midi_keys::synths())
}

#[cfg(windows)]
fn main() {
    use solplugs_midi_keys::{TrackCfg, devices};
    use std::sync::mpsc;
    let (act_tx, act_rx) = mpsc::channel::<devices::Activity>();
    std::thread::spawn(move || {
        while let Ok(a) = act_rx.recv() {
            say(&json!({ "ev": "note", "track": a.track, "note": a.note, "on": a.on }));
        }
    });
    let mut rig: Option<devices::Rig> = None;
    say(&json!({ "ev": "ready", "version": env!("CARGO_PKG_VERSION") }));
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            say(&json!({ "ok": false, "error": "not JSON" }));
            continue;
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let op = req.get("op").and_then(Value::as_str).unwrap_or("");
        let answer = match op {
            "list" => {
                json!({ "ok": true, "inputs": devices::midi_inputs(), "output": devices::output_name(), "synths": synth_list() })
            }
            "apply" => {
                let tracks: Vec<TrackCfg> = req
                    .get("tracks")
                    .cloned()
                    .and_then(|t| serde_json::from_value(t).ok())
                    .unwrap_or_default();
                let buffer = req
                    .get("buffer")
                    .and_then(Value::as_u64)
                    .and_then(|b| u32::try_from(b).ok())
                    .unwrap_or(128);
                if rig.is_none() {
                    match devices::Rig::open(buffer, act_tx.clone()) {
                        Ok(r) => rig = Some(r),
                        Err(e) => {
                            say(&json!({ "id": id, "ok": false, "error": e }));
                            continue;
                        }
                    }
                }
                match rig.as_mut() {
                    Some(r) => {
                        let states = r.apply(&tracks);
                        json!({ "ok": true, "tracks": states, "output": r.device, "buffer": r.buffer_frames, "latency_ms": r.latency_ms(), "inputs_connected": r.connected() })
                    }
                    None => json!({ "ok": false, "error": "no audio output" }),
                }
            }
            "note" => match rig.as_mut() {
                Some(r) => {
                    let n = |k: &str, d: u64| req.get(k).and_then(Value::as_u64).unwrap_or(d);
                    r.note(
                        usize::try_from(n("track", 0)).unwrap_or(0),
                        u8::try_from(n("note", 60)).unwrap_or(60),
                        u8::try_from(n("velocity", 100)).unwrap_or(100),
                        req.get("on").and_then(Value::as_bool).unwrap_or(true),
                    );
                    json!({ "ok": true })
                }
                None => json!({ "ok": false, "error": "Pick a synth first." }),
            },
            "panic" => {
                if let Some(r) = rig.as_mut() {
                    r.panic();
                }
                json!({ "ok": true })
            }
            "stop" => break,
            _ => json!({ "ok": false, "error": format!("unknown op {op:?}") }),
        };
        let mut a = answer;
        if let Some(o) = a.as_object_mut() {
            o.insert("id".into(), id);
        }
        say(&a);
        if let Some(r) = rig.as_mut() {
            r.drain();
        }
    }
}

#[cfg(not(windows))]
fn main() {
    // The devices are Windows only for now; the synth list still works (for tests).
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let req: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        match req.get("op").and_then(Value::as_str) {
            Some("list") => say(
                &json!({ "id": id, "ok": true, "inputs": [], "output": null, "synths": synth_list() }),
            ),
            Some("stop") => break,
            _ => say(
                &json!({ "id": id, "ok": false, "error": "MIDI and audio devices are supported on Windows only" }),
            ),
        }
    }
}
