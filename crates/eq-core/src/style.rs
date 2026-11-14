//! The eight styles: their names, the knobs each faceplate has, and how those knobs become
//! filter bands. The curves follow how the classic units behave (named by type, never by
//! brand): Pultec's broad tube shelves and the boost-and-cut trick, API's proportional Q,
//! SSL's clean parametric, Neve's warm fixed-frequency bands, the passive mastering curves,
//! the Air Band, and the Abbey Road console's wide musical curves.

use crate::filter::{BandSpec, Shape};

/// An EQ style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Style {
    /// Smooth broad tube shelves, the boost-and-cut trick.
    #[default]
    Pultec = 0,
    /// Punchy, proportional Q.
    Api = 1,
    /// Clean console parametric.
    Ssl = 2,
    /// Warm Class A colour.
    Neve = 3,
    /// Transparent modern digital: up to 24 bands.
    Clean = 4,
    /// Mastering tube EQ, passive curves.
    Massive = 5,
    /// The Air Band.
    Maag = 6,
    /// Wide musical console curves.
    Curve = 7,
}

impl Style {
    /// All styles, in parameter order.
    pub const ALL: [Style; 8] = [
        Style::Pultec,
        Style::Api,
        Style::Ssl,
        Style::Neve,
        Style::Clean,
        Style::Massive,
        Style::Maag,
        Style::Curve,
    ];

    /// Number of styles.
    pub const COUNT: usize = 8;

    /// Index.
    pub fn index(self) -> usize {
        self as usize
    }

    /// From an index (Pultec for out of range).
    pub fn from_index(i: usize) -> Style {
        Style::ALL.get(i).copied().unwrap_or_default()
    }

    /// Display name: the type, never a brand.
    pub fn name(self) -> &'static str {
        match self {
            Style::Pultec => "Pultec",
            Style::Api => "API",
            Style::Ssl => "SSL",
            Style::Neve => "Neve",
            Style::Clean => "Clean",
            Style::Massive => "Massive",
            Style::Maag => "Maag",
            Style::Curve => "Curve",
        }
    }

    /// Whether the style has analogue colour (oversampled saturation).
    pub fn coloured(self) -> bool {
        self != Style::Clean
    }

    /// Saturation drive and bias (asymmetry: even harmonics) of the style's colour stage.
    pub fn colour(self) -> (f32, f32) {
        match self {
            // Tube make-up amp: soft and lopsided (second harmonic).
            Style::Pultec => (0.75, 0.28),
            // Discrete op-amp: tighter, mostly odd, punchy.
            Style::Api => (0.95, 0.06),
            // Console VCA/op-amp path: barely there until pushed.
            Style::Ssl => (0.3, 0.03),
            // Class A + transformers: warm, thick.
            Style::Neve => (1.05, 0.2),
            Style::Clean => (0.0, 0.0),
            // Tube mastering: rich but smooth.
            Style::Massive => (0.6, 0.22),
            // Clean, open.
            Style::Maag => (0.22, 0.05),
            // Valve + transformer console: bold.
            Style::Curve => (0.9, 0.16),
        }
    }

    /// The style's knobs (Clean has none: it uses bands).
    pub fn knobs(self) -> &'static [Knob] {
        match self {
            Style::Pultec => PULTEC,
            Style::Api => API,
            Style::Ssl => SSL,
            Style::Neve => NEVE,
            Style::Clean => &[],
            Style::Massive => MASSIVE,
            Style::Maag => MAAG,
            Style::Curve => CURVE,
        }
    }

    /// Default knob values.
    pub fn defaults(self) -> [f32; MAX_KNOBS] {
        let mut v = [0.0; MAX_KNOBS];
        for (slot, k) in v.iter_mut().zip(self.knobs()) {
            *slot = k.default;
        }
        v
    }
}

/// Most knobs a style has.
pub const MAX_KNOBS: usize = 24;
/// Most bands a hardware style designs.
pub const MAX_HW_BANDS: usize = 8;

/// How a knob's value reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unit {
    /// dB, signed.
    Db,
    /// Hz.
    Hz,
    /// A plain number (0-10 hardware scales).
    Number,
    /// Q.
    Q,
    /// A pick from a list (the value is the index).
    Choice(&'static [&'static str]),
    /// A switch: (off text, on text).
    Switch(&'static str, &'static str),
}

/// One faceplate knob or switch.
#[derive(Clone, Copy, Debug)]
pub struct Knob {
    /// Name ("Low Boost").
    pub name: &'static str,
    /// Short name for hosts.
    pub short: &'static str,
    /// Lowest value.
    pub min: f32,
    /// Highest value.
    pub max: f32,
    /// Default.
    pub default: f32,
    /// Logarithmic travel (frequencies).
    pub log: bool,
    /// How it reads.
    pub unit: Unit,
    /// Help shown on hover.
    pub help: &'static str,
}

impl Knob {
    /// Number of steps (0 = continuous).
    pub fn steps(&self) -> u32 {
        match self.unit {
            Unit::Choice(c) => c.len() as u32,
            Unit::Switch(..) => 2,
            _ => 0,
        }
    }
}

const fn db(name: &'static str, short: &'static str, range: f32, help: &'static str) -> Knob {
    Knob {
        name,
        short,
        min: -range,
        max: range,
        default: 0.0,
        log: false,
        unit: Unit::Db,
        help,
    }
}

const fn hz(
    name: &'static str,
    short: &'static str,
    (min, max, default): (f32, f32, f32),
    help: &'static str,
) -> Knob {
    Knob {
        name,
        short,
        min,
        max,
        default,
        log: true,
        unit: Unit::Hz,
        help,
    }
}

const fn choice(
    name: &'static str,
    short: &'static str,
    list: &'static [&'static str],
    default: f32,
    help: &'static str,
) -> Knob {
    Knob {
        name,
        short,
        min: 0.0,
        max: (list.len() - 1) as f32,
        default,
        log: false,
        unit: Unit::Choice(list),
        help,
    }
}

const fn switch(
    name: &'static str,
    short: &'static str,
    texts: (&'static str, &'static str),
    default: f32,
    help: &'static str,
) -> Knob {
    Knob {
        name,
        short,
        min: 0.0,
        max: 1.0,
        default,
        log: false,
        unit: Unit::Switch(texts.0, texts.1),
        help,
    }
}

const fn ten(name: &'static str, short: &'static str, default: f32, help: &'static str) -> Knob {
    Knob {
        name,
        short,
        min: 0.0,
        max: 10.0,
        default,
        log: false,
        unit: Unit::Number,
        help,
    }
}

/// Parses a choice label like "60", "1.6k", "Off" into Hz (0 for "Off").
pub fn choice_hz(label: &str) -> f64 {
    let l = label.trim();
    if let Some(k) = l.strip_suffix('k') {
        k.parse::<f64>().map_or(0.0, |v| v * 1000.0)
    } else {
        l.parse::<f64>().unwrap_or(0.0)
    }
}

fn pick(list: &[&str], v: f32) -> f64 {
    let i = (v.round().max(0.0) as usize).min(list.len().saturating_sub(1));
    list.get(i).map_or(0.0, |l| choice_hz(l))
}

// ------------------------------------------------------------------ Pultec

/// Pultec knob indices.
pub mod pultec {
    /// Low frequency (choice).
    pub const LOW_FREQ: usize = 0;
    /// Low boost 0-10.
    pub const LOW_BOOST: usize = 1;
    /// Low attenuation 0-10.
    pub const LOW_ATTEN: usize = 2;
    /// High boost frequency (choice).
    pub const HIGH_FREQ: usize = 3;
    /// High boost 0-10.
    pub const HIGH_BOOST: usize = 4;
    /// Bandwidth 0 (sharp) - 10 (broad).
    pub const BANDWIDTH: usize = 5;
    /// High attenuation frequency (choice).
    pub const ATTEN_FREQ: usize = 6;
    /// High attenuation 0-10.
    pub const HIGH_ATTEN: usize = 7;
    /// Low frequencies.
    pub const LOW_FREQS: &[&str] = &["20", "30", "60", "100"];
    /// High boost frequencies.
    pub const HIGH_FREQS: &[&str] = &["3k", "4k", "5k", "8k", "10k", "12k", "16k"];
    /// High attenuation frequencies.
    pub const ATTEN_FREQS: &[&str] = &["5k", "10k", "20k"];
}

const PULTEC: &[Knob] = &[
    choice(
        "Low Frequency",
        "Low Hz",
        pultec::LOW_FREQS,
        2.0,
        "Where the low boost and attenuation sit.",
    ),
    ten(
        "Low Boost",
        "Low Bst",
        0.0,
        "Broad, slightly resonant low shelf, up to +13.5 dB. Turn Low Atten up too for the famous trick: big lows with the mud scooped out above them.",
    ),
    ten(
        "Low Atten",
        "Low Att",
        0.0,
        "Broad low cut (up to -9 dB) that reaches higher than the boost. With the boost: the boost-and-cut trick.",
    ),
    choice(
        "High Frequency",
        "Hi Hz",
        pultec::HIGH_FREQS,
        4.0,
        "Where the high boost peaks.",
    ),
    ten(
        "High Boost",
        "Hi Bst",
        0.0,
        "Smooth high peak, up to about +16 dB: air and presence without edge.",
    ),
    ten(
        "Bandwidth",
        "BW",
        5.0,
        "Width of the high boost: 0 = sharp, 10 = broad.",
    ),
    choice(
        "High Atten Frequency",
        "Att Hz",
        pultec::ATTEN_FREQS,
        1.0,
        "Where the high shelf cut starts.",
    ),
    ten(
        "High Atten",
        "Hi Att",
        0.0,
        "Gentle high shelf cut, up to about -16 dB: tames hiss and fizz.",
    ),
];

fn pultec_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use pultec::*;
    let lf = pick(LOW_FREQS, k[LOW_FREQ]);
    let boost = f64::from(k[LOW_BOOST].clamp(0.0, 10.0)) * 1.35;
    let atten = f64::from(k[LOW_ATTEN].clamp(0.0, 10.0)) * 0.9;
    // Boost: a resonant shelf with its bump around the chosen frequency.
    out[0] = BandSpec::shelf(false, lf * 2.0, boost, 1.0).with_on(boost > 0.0);
    // Attenuation: a broad shelf whose transition sits well above the boost, so the two
    // together lift the lows and dip the low mids (the boost-and-cut trick).
    out[1] = BandSpec::shelf(false, lf * 5.0, -atten, 0.5).with_on(atten > 0.0);
    let hf = pick(HIGH_FREQS, k[HIGH_FREQ]);
    let hb = f64::from(k[HIGH_BOOST].clamp(0.0, 10.0)) * 1.6;
    let bw = f64::from(k[BANDWIDTH].clamp(0.0, 10.0)) / 10.0;
    let q = 2.4 * (0.45f64 / 2.4).powf(bw);
    // The boost widens slightly as it grows (passive LC network).
    let q = q / (1.0 + hb / 40.0);
    out[2] = BandSpec::bell(hf, hb, q).with_on(hb > 0.0);
    let af = pick(ATTEN_FREQS, k[ATTEN_FREQ]);
    let ha = f64::from(k[HIGH_ATTEN].clamp(0.0, 10.0)) * 1.6;
    out[3] = BandSpec::shelf(true, af, -ha, 0.55).with_on(ha > 0.0);
}

// ------------------------------------------------------------------ API

/// API knob indices.
pub mod api {
    /// Low frequency (choice).
    pub const LOW_FREQ: usize = 0;
    /// Low gain, dB.
    pub const LOW_GAIN: usize = 1;
    /// Low shelf (else peak).
    pub const LOW_SHELF: usize = 2;
    /// Mid frequency (choice).
    pub const MID_FREQ: usize = 3;
    /// Mid gain, dB.
    pub const MID_GAIN: usize = 4;
    /// High frequency (choice).
    pub const HIGH_FREQ: usize = 5;
    /// High gain, dB.
    pub const HIGH_GAIN: usize = 6;
    /// High shelf (else peak).
    pub const HIGH_SHELF: usize = 7;
    /// Band-pass filter (50 Hz high-pass, 15 kHz low-pass).
    pub const FILTER: usize = 8;
    /// Low frequencies.
    pub const LOW_FREQS: &[&str] = &["30", "40", "50", "100", "200", "300", "400"];
    /// Mid frequencies.
    pub const MID_FREQS: &[&str] = &["200", "400", "800", "1.5k", "3k", "5k", "8k"];
    /// High frequencies.
    pub const HIGH_FREQS: &[&str] = &["2.5k", "5k", "7.5k", "10k", "12.5k", "15k", "20k"];
}

const API_GAIN: f32 = 12.0;

const API: &[Knob] = &[
    choice(
        "Low Frequency",
        "Low Hz",
        api::LOW_FREQS,
        3.0,
        "Low band frequency.",
    ),
    db(
        "Low Gain",
        "Low",
        API_GAIN,
        "Low band boost or cut. Proportional Q: the more you boost, the tighter it gets.",
    ),
    switch(
        "Low Shelf",
        "Low Shf",
        ("Peak", "Shelf"),
        1.0,
        "Low band as a shelf or a peak.",
    ),
    choice(
        "Mid Frequency",
        "Mid Hz",
        api::MID_FREQS,
        3.0,
        "Mid band frequency.",
    ),
    db(
        "Mid Gain",
        "Mid",
        API_GAIN,
        "Mid band boost or cut. Narrows as you push it: punch without mud.",
    ),
    choice(
        "High Frequency",
        "Hi Hz",
        api::HIGH_FREQS,
        3.0,
        "High band frequency.",
    ),
    db("High Gain", "High", API_GAIN, "High band boost or cut."),
    switch(
        "High Shelf",
        "Hi Shf",
        ("Peak", "Shelf"),
        1.0,
        "High band as a shelf or a peak.",
    ),
    switch(
        "Filter",
        "Filt",
        ("Off", "On"),
        0.0,
        "Band-pass filter: rolls off below 50 Hz and above 15 kHz.",
    ),
];

/// Proportional Q: wide for small moves, tight for big ones.
fn proportional_q(gain: f64, range: f64, wide: f64, tight: f64) -> f64 {
    let t = (gain.abs() / range).clamp(0.0, 1.0);
    wide + (tight - wide) * t
}

fn api_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use api::*;
    let r = f64::from(API_GAIN);
    let lg = f64::from(k[LOW_GAIN]);
    let lf = pick(LOW_FREQS, k[LOW_FREQ]);
    out[0] = if k[LOW_SHELF] >= 0.5 {
        BandSpec::shelf(false, lf, lg, proportional_q(lg, r, 0.6, 1.0))
    } else {
        BandSpec::bell(lf, lg, proportional_q(lg, r, 0.5, 2.2))
    };
    let mg = f64::from(k[MID_GAIN]);
    out[1] = BandSpec::bell(
        pick(MID_FREQS, k[MID_FREQ]),
        mg,
        proportional_q(mg, r, 0.45, 2.4),
    );
    let hg = f64::from(k[HIGH_GAIN]);
    let hf = pick(HIGH_FREQS, k[HIGH_FREQ]);
    out[2] = if k[HIGH_SHELF] >= 0.5 {
        BandSpec::shelf(true, hf, hg, proportional_q(hg, r, 0.6, 1.0))
    } else {
        BandSpec::bell(hf, hg, proportional_q(hg, r, 0.5, 2.2))
    };
    let filt = k[FILTER] >= 0.5;
    out[3] = BandSpec::cut(false, 50.0, 12).with_on(filt);
    out[4] = BandSpec::cut(true, 15_000.0, 12).with_on(filt);
}

// ------------------------------------------------------------------ SSL

/// SSL knob indices.
pub mod ssl {
    /// High-pass, Hz (lowest = off).
    pub const HPF: usize = 0;
    /// Low-pass, Hz (highest = off).
    pub const LPF: usize = 1;
    /// LF gain.
    pub const LF_GAIN: usize = 2;
    /// LF frequency.
    pub const LF_FREQ: usize = 3;
    /// LF bell (else shelf).
    pub const LF_BELL: usize = 4;
    /// LMF gain.
    pub const LMF_GAIN: usize = 5;
    /// LMF frequency.
    pub const LMF_FREQ: usize = 6;
    /// LMF Q.
    pub const LMF_Q: usize = 7;
    /// HMF gain.
    pub const HMF_GAIN: usize = 8;
    /// HMF frequency.
    pub const HMF_FREQ: usize = 9;
    /// HMF Q.
    pub const HMF_Q: usize = 10;
    /// HF gain.
    pub const HF_GAIN: usize = 11;
    /// HF frequency.
    pub const HF_FREQ: usize = 12;
    /// HF bell (else shelf).
    pub const HF_BELL: usize = 13;
    /// HPF at or below this is off.
    pub const HPF_OFF: f32 = 16.0;
    /// LPF at or above this is off.
    pub const LPF_OFF: f32 = 22_000.0;
}

const fn q_knob(name: &'static str, short: &'static str, help: &'static str) -> Knob {
    Knob {
        name,
        short,
        min: 0.5,
        max: 2.5,
        default: 1.0,
        log: true,
        unit: Unit::Q,
        help,
    }
}

const SSL: &[Knob] = &[
    hz(
        "High Pass",
        "HPF",
        (16.0, 350.0, 16.0),
        "18 dB/oct high-pass. All the way down is off.",
    ),
    hz(
        "Low Pass",
        "LPF",
        (3000.0, 22_000.0, 22_000.0),
        "12 dB/oct low-pass. All the way up is off.",
    ),
    db("LF Gain", "LF", 15.0, "Low band gain."),
    hz(
        "LF Frequency",
        "LF Hz",
        (30.0, 450.0, 100.0),
        "Low band frequency.",
    ),
    switch(
        "LF Bell",
        "LF Bel",
        ("Shelf", "Bell"),
        0.0,
        "Low band as a shelf or a bell.",
    ),
    db("LMF Gain", "LMF", 15.0, "Low-mid gain."),
    hz(
        "LMF Frequency",
        "LMF Hz",
        (200.0, 2500.0, 600.0),
        "Low-mid frequency.",
    ),
    q_knob("LMF Q", "LMF Q", "Low-mid width: higher is narrower."),
    db("HMF Gain", "HMF", 15.0, "High-mid gain."),
    hz(
        "HMF Frequency",
        "HMF Hz",
        (600.0, 7000.0, 2000.0),
        "High-mid frequency.",
    ),
    q_knob("HMF Q", "HMF Q", "High-mid width: higher is narrower."),
    db("HF Gain", "HF", 15.0, "High band gain."),
    hz(
        "HF Frequency",
        "HF Hz",
        (1500.0, 16_000.0, 8000.0),
        "High band frequency.",
    ),
    switch(
        "HF Bell",
        "HF Bel",
        ("Shelf", "Bell"),
        0.0,
        "High band as a shelf or a bell.",
    ),
];

fn ssl_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use ssl::*;
    let f = |i: usize| f64::from(k[i]);
    out[0] = BandSpec::cut(false, f(HPF), 18).with_on(k[HPF] > HPF_OFF + 0.5);
    out[1] = BandSpec::cut(true, f(LPF), 12).with_on(k[LPF] < LPF_OFF - 1.0);
    // Mildly gain-dependent Q, as on the console: clean and precise, never muddy.
    let pq = |g: f64, q: f64| q * (0.75 + 0.5 * (g.abs() / 15.0).min(1.0));
    out[2] = if k[LF_BELL] >= 0.5 {
        BandSpec::bell(f(LF_FREQ), f(LF_GAIN), pq(f(LF_GAIN), 0.9))
    } else {
        BandSpec::shelf(false, f(LF_FREQ), f(LF_GAIN), 0.707)
    };
    out[3] = BandSpec::bell(f(LMF_FREQ), f(LMF_GAIN), pq(f(LMF_GAIN), f(LMF_Q)));
    out[4] = BandSpec::bell(f(HMF_FREQ), f(HMF_GAIN), pq(f(HMF_GAIN), f(HMF_Q)));
    out[5] = if k[HF_BELL] >= 0.5 {
        BandSpec::bell(f(HF_FREQ), f(HF_GAIN), pq(f(HF_GAIN), 0.9))
    } else {
        BandSpec::shelf(true, f(HF_FREQ), f(HF_GAIN), 0.707)
    };
}

// ------------------------------------------------------------------ Neve

/// Neve knob indices.
pub mod neve {
    /// High-pass (choice, first = off).
    pub const HPF: usize = 0;
    /// Low shelf frequency (choice).
    pub const LOW_FREQ: usize = 1;
    /// Low shelf gain.
    pub const LOW_GAIN: usize = 2;
    /// Mid frequency (choice).
    pub const MID_FREQ: usize = 3;
    /// Mid gain.
    pub const MID_GAIN: usize = 4;
    /// High shelf gain (12 kHz).
    pub const HIGH_GAIN: usize = 5;
    /// High-pass choices.
    pub const HPFS: &[&str] = &["Off", "50", "80", "160", "300"];
    /// Low shelf frequencies.
    pub const LOW_FREQS: &[&str] = &["35", "60", "110", "220"];
    /// Mid frequencies.
    pub const MID_FREQS: &[&str] = &["360", "700", "1.6k", "3.2k", "4.8k", "7.2k"];
}

const NEVE: &[Knob] = &[
    choice(
        "High Pass",
        "HPF",
        neve::HPFS,
        0.0,
        "18 dB/oct high-pass: cleans up rumble before the warm stages.",
    ),
    choice(
        "Low Frequency",
        "Low Hz",
        neve::LOW_FREQS,
        2.0,
        "Low shelf frequency.",
    ),
    db(
        "Low Gain",
        "Low",
        16.0,
        "Low shelf: big round low end, with the gentle bump of the original.",
    ),
    choice(
        "Mid Frequency",
        "Mid Hz",
        neve::MID_FREQS,
        2.0,
        "Mid bell frequency.",
    ),
    db(
        "Mid Gain",
        "Mid",
        18.0,
        "Broad mid bell: presence and body.",
    ),
    db("High Gain", "High", 16.0, "12 kHz shelf: sweet top end."),
];

fn neve_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use neve::*;
    let hp = pick(HPFS, k[HPF]);
    out[0] = BandSpec::cut(false, if hp > 0.0 { hp } else { 50.0 }, 18).with_on(hp > 0.0);
    let lg = f64::from(k[LOW_GAIN]);
    // The inductor shelf overshoots a little: Q above 0.707 gives the bump and dip.
    out[1] = BandSpec::shelf(false, pick(LOW_FREQS, k[LOW_FREQ]), lg, 0.9);
    let mg = f64::from(k[MID_GAIN]);
    out[2] = BandSpec::bell(
        pick(MID_FREQS, k[MID_FREQ]),
        mg,
        proportional_q(mg, 18.0, 0.7, 1.3),
    );
    out[3] = BandSpec::shelf(true, 12_000.0, f64::from(k[HIGH_GAIN]), 0.6);
}

// ------------------------------------------------------------------ Massive

/// Massive knob indices: four bands of four knobs, then the filters.
pub mod massive {
    /// Band `b` frequency.
    pub const fn freq(b: usize) -> usize {
        b * 4
    }
    /// Band `b` gain.
    pub const fn gain(b: usize) -> usize {
        b * 4 + 1
    }
    /// Band `b` bandwidth 0-10.
    pub const fn bandwidth(b: usize) -> usize {
        b * 4 + 2
    }
    /// Band `b` shelf (else bell).
    pub const fn shelf(b: usize) -> usize {
        b * 4 + 3
    }
    /// High-pass (choice).
    pub const HPF: usize = 16;
    /// Low-pass (choice).
    pub const LPF: usize = 17;
    /// High-pass choices.
    pub const HPFS: &[&str] = &["Off", "22", "39", "68"];
    /// Low-pass choices.
    pub const LPFS: &[&str] = &["Off", "27k", "18k", "12k"];
}

const fn mband(
    freq: &'static str,
    gain: &'static str,
    bw: &'static str,
    shelf: &'static str,
    (min, max, default): (f32, f32, f32),
    is_shelf: f32,
) -> [Knob; 4] {
    [
        hz(freq, freq, (min, max, default), "Band frequency."),
        db(
            gain,
            gain,
            20.0,
            "Boost or cut. Passive curves: wide and smooth even when pushed.",
        ),
        ten(
            bw,
            bw,
            5.0,
            "Bandwidth: 0 narrow, 10 wide. On a shelf it sets the bump and dip at the corner.",
        ),
        switch(shelf, shelf, ("Bell", "Shelf"), is_shelf, "Shelf or bell."),
    ]
}

const MASSIVE_BANDS: [[Knob; 4]; 4] = [
    mband(
        "Band 1 Freq",
        "B1 Gain",
        "B1 Width",
        "B1 Shelf",
        (22.0, 1000.0, 60.0),
        1.0,
    ),
    mband(
        "Band 2 Freq",
        "B2 Gain",
        "B2 Width",
        "B2 Shelf",
        (82.0, 3900.0, 400.0),
        0.0,
    ),
    mband(
        "Band 3 Freq",
        "B3 Gain",
        "B3 Width",
        "B3 Shelf",
        (220.0, 10_000.0, 2200.0),
        0.0,
    ),
    mband(
        "Band 4 Freq",
        "B4 Gain",
        "B4 Width",
        "B4 Shelf",
        (560.0, 27_000.0, 10_000.0),
        1.0,
    ),
];

const MASSIVE: &[Knob] = &[
    MASSIVE_BANDS[0][0],
    MASSIVE_BANDS[0][1],
    MASSIVE_BANDS[0][2],
    MASSIVE_BANDS[0][3],
    MASSIVE_BANDS[1][0],
    MASSIVE_BANDS[1][1],
    MASSIVE_BANDS[1][2],
    MASSIVE_BANDS[1][3],
    MASSIVE_BANDS[2][0],
    MASSIVE_BANDS[2][1],
    MASSIVE_BANDS[2][2],
    MASSIVE_BANDS[2][3],
    MASSIVE_BANDS[3][0],
    MASSIVE_BANDS[3][1],
    MASSIVE_BANDS[3][2],
    MASSIVE_BANDS[3][3],
    choice(
        "High Pass",
        "HPF",
        massive::HPFS,
        0.0,
        "Gentle 12 dB/oct high-pass.",
    ),
    choice(
        "Low Pass",
        "LPF",
        massive::LPFS,
        0.0,
        "Gentle 12 dB/oct low-pass.",
    ),
];

fn massive_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use massive::*;
    for b in 0..4 {
        let f = f64::from(k[freq(b)]);
        let g = f64::from(k[gain(b)]);
        let w = f64::from(k[bandwidth(b)].clamp(0.0, 10.0)) / 10.0;
        out[b] = if k[shelf(b)] >= 0.5 {
            // Narrow bandwidth: a resonant shelf with bump and dip; wide: a soft slope.
            let q = 1.6 * (0.5f64 / 1.6).powf(w);
            BandSpec::shelf(b >= 2, f, g, q)
        } else {
            // Passive bells stay broad; cuts are a touch narrower than boosts.
            let q = 2.0 * (0.35f64 / 2.0).powf(w) * if g < 0.0 { 1.2 } else { 1.0 };
            BandSpec::bell(f, g, q)
        };
    }
    let hp = pick(HPFS, k[HPF]);
    out[4] = BandSpec::cut(false, if hp > 0.0 { hp } else { 22.0 }, 12).with_on(hp > 0.0);
    let lp = pick(LPFS, k[LPF]);
    out[5] = BandSpec::cut(true, if lp > 0.0 { lp } else { 27_000.0 }, 12).with_on(lp > 0.0);
}

// ------------------------------------------------------------------ Maag

/// Maag knob indices.
pub mod maag {
    /// Air band frequency (choice).
    pub const AIR_FREQ: usize = 0;
    /// Air band gain (boost only).
    pub const AIR_GAIN: usize = 1;
    /// Low frequency.
    pub const LOW_FREQ: usize = 2;
    /// Low gain.
    pub const LOW_GAIN: usize = 3;
    /// Mid frequency.
    pub const MID_FREQ: usize = 4;
    /// Mid gain.
    pub const MID_GAIN: usize = 5;
    /// 80 Hz high-pass.
    pub const HPF: usize = 6;
    /// Air band frequencies.
    pub const AIR_FREQS: &[&str] = &["2.5k", "5k", "10k", "20k", "40k"];
}

const MAAG: &[Knob] = &[
    choice(
        "Air Frequency",
        "Air Hz",
        maag::AIR_FREQS,
        4.0,
        "Air Band frequency. 40 kHz lifts the top gently from far above hearing: air without harshness.",
    ),
    Knob {
        name: "Air Band",
        short: "Air",
        min: 0.0,
        max: 15.0,
        default: 0.0,
        log: false,
        unit: Unit::Db,
        help: "The Air Band: a very wide, boost-only high shelf. Sparkle and openness without edge.",
    },
    hz(
        "Low Frequency",
        "Low Hz",
        (40.0, 400.0, 100.0),
        "Low band frequency.",
    ),
    db("Low Gain", "Low", 10.0, "Broad low band."),
    hz(
        "Mid Frequency",
        "Mid Hz",
        (300.0, 5000.0, 1200.0),
        "Mid band frequency.",
    ),
    db("Mid Gain", "Mid", 10.0, "Broad mid band."),
    switch(
        "High Pass",
        "HPF",
        ("Off", "80 Hz"),
        0.0,
        "80 Hz high-pass.",
    ),
];

fn maag_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use maag::*;
    let af = pick(AIR_FREQS, k[AIR_FREQ]);
    let ag = f64::from(k[AIR_GAIN].clamp(0.0, 15.0));
    // A very wide, soft shelf: the lift starts octaves below the nominal frequency, so even
    // 40 kHz is heard as air (about a third of the gain at 16 kHz), with no peak.
    out[0] = BandSpec::shelf(true, af * 0.625, ag, 0.4).with_on(ag > 0.0);
    out[1] = BandSpec::bell(f64::from(k[LOW_FREQ]), f64::from(k[LOW_GAIN]), 0.6);
    out[2] = BandSpec::bell(f64::from(k[MID_FREQ]), f64::from(k[MID_GAIN]), 0.7);
    out[3] = BandSpec::cut(false, 80.0, 12).with_on(k[HPF] >= 0.5);
}

// ------------------------------------------------------------------ Curve

/// Curve knob indices.
pub mod curve {
    /// Bass frequency (choice).
    pub const BASS_FREQ: usize = 0;
    /// Bass gain.
    pub const BASS_GAIN: usize = 1;
    /// Bass shelf (else bell).
    pub const BASS_SHELF: usize = 2;
    /// Mid frequency (choice).
    pub const MID_FREQ: usize = 3;
    /// Mid gain.
    pub const MID_GAIN: usize = 4;
    /// Treble frequency (choice).
    pub const TREBLE_FREQ: usize = 5;
    /// Treble gain.
    pub const TREBLE_GAIN: usize = 6;
    /// Treble shelf (else bell).
    pub const TREBLE_SHELF: usize = 7;
    /// High-pass (choice).
    pub const HPF: usize = 8;
    /// Bass frequencies.
    pub const BASS_FREQS: &[&str] = &["30", "47", "68", "100", "150", "220", "330"];
    /// Mid frequencies.
    pub const MID_FREQS: &[&str] = &[
        "220", "330", "470", "680", "1k", "1.5k", "2.2k", "3.3k", "4.7k", "6.8k",
    ];
    /// Treble frequencies.
    pub const TREBLE_FREQS: &[&str] = &["2.7k", "3.9k", "5.6k", "8.2k", "12k", "16k"];
    /// High-pass choices.
    pub const HPFS: &[&str] = &["Off", "30", "60", "120"];
}

const CURVE: &[Knob] = &[
    choice(
        "Bass Frequency",
        "Bass Hz",
        curve::BASS_FREQS,
        3.0,
        "Bass band frequency.",
    ),
    db("Bass", "Bass", 15.0, "Bold, wide bass curve."),
    switch(
        "Bass Shelf",
        "Bass Sh",
        ("Bell", "Shelf"),
        1.0,
        "Bass band as a shelf or a bell.",
    ),
    choice(
        "Mid Frequency",
        "Mid Hz",
        curve::MID_FREQS,
        4.0,
        "Mid band frequency.",
    ),
    db("Mid", "Mid", 15.0, "Wide, musical mid curve."),
    choice(
        "Treble Frequency",
        "Treb Hz",
        curve::TREBLE_FREQS,
        3.0,
        "Treble band frequency.",
    ),
    db("Treble", "Treb", 15.0, "Bold treble curve."),
    switch(
        "Treble Shelf",
        "Treb Sh",
        ("Bell", "Shelf"),
        1.0,
        "Treble band as a shelf or a bell.",
    ),
    choice("High Pass", "HPF", curve::HPFS, 0.0, "12 dB/oct high-pass."),
];

fn curve_bands(k: &[f32], out: &mut [BandSpec; MAX_HW_BANDS]) {
    use curve::*;
    let bg = f64::from(k[BASS_GAIN]);
    let bf = pick(BASS_FREQS, k[BASS_FREQ]);
    // Console inductor curves: the bass shelf overshoots into a bold bump, the treble
    // shelf starts early and rises slowly. Wide and musical rather than precise.
    out[0] = if k[BASS_SHELF] >= 0.5 {
        BandSpec::shelf(false, bf * 1.3, bg, 1.15)
    } else {
        BandSpec::bell(bf, bg, 0.55)
    };
    out[1] = BandSpec::bell(pick(MID_FREQS, k[MID_FREQ]), f64::from(k[MID_GAIN]), 0.6);
    let tg = f64::from(k[TREBLE_GAIN]);
    let tf = pick(TREBLE_FREQS, k[TREBLE_FREQ]);
    out[2] = if k[TREBLE_SHELF] >= 0.5 {
        BandSpec::shelf(true, tf * 0.7, tg, 0.5)
    } else {
        BandSpec::bell(tf, tg, 0.55)
    };
    let hp = pick(HPFS, k[HPF]);
    out[3] = BandSpec::cut(false, if hp > 0.0 { hp } else { 30.0 }, 12).with_on(hp > 0.0);
}

/// The bands a hardware style's knobs make. Slots a style doesn't use stay off. Clean
/// returns all off (it has its own bands).
pub fn hardware_bands(style: Style, knobs: &[f32; MAX_KNOBS]) -> [BandSpec; MAX_HW_BANDS] {
    let mut out = [BandSpec::default(); MAX_HW_BANDS];
    match style {
        Style::Pultec => pultec_bands(knobs, &mut out),
        Style::Api => api_bands(knobs, &mut out),
        Style::Ssl => ssl_bands(knobs, &mut out),
        Style::Neve => neve_bands(knobs, &mut out),
        Style::Clean => {}
        Style::Massive => massive_bands(knobs, &mut out),
        Style::Maag => maag_bands(knobs, &mut out),
        Style::Curve => curve_bands(knobs, &mut out),
    }
    out
}

/// Whether band `b` of a hardware design is a cut filter (drawn differently).
pub fn is_cut(spec: &BandSpec) -> bool {
    matches!(spec.shape, Shape::LowCut | Shape::HighCut)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 192_000.0;

    fn total_db(style: Style, k: &[f32; MAX_KNOBS], f: f64) -> f64 {
        hardware_bands(style, k)
            .iter()
            .map(|b| b.magnitude_db(f, FS))
            .sum()
    }

    #[test]
    fn knob_tables_are_sane() {
        for s in Style::ALL {
            assert!(s.knobs().len() <= MAX_KNOBS);
            for k in s.knobs() {
                assert!(k.min < k.max, "{} {}", s.name(), k.name);
                assert!(k.default >= k.min && k.default <= k.max, "{}", k.name);
                if k.log {
                    assert!(k.min > 0.0);
                }
            }
        }
    }

    #[test]
    fn defaults_are_flat() {
        for s in Style::ALL {
            let k = s.defaults();
            for f in [30.0, 100.0, 1000.0, 5000.0, 15_000.0] {
                let db = total_db(s, &k, f);
                assert!(db.abs() < 0.01, "{} at {f}: {db}", s.name());
            }
        }
    }

    #[test]
    fn pultec_boost_and_cut_trick() {
        let mut k = Style::Pultec.defaults();
        k[pultec::LOW_FREQ] = 2.0; // 60 Hz
        k[pultec::LOW_BOOST] = 10.0;
        k[pultec::LOW_ATTEN] = 10.0;
        let low = total_db(Style::Pultec, &k, 50.0);
        let dip = (150..600)
            .step_by(10)
            .map(|f| total_db(Style::Pultec, &k, f64::from(f)))
            .fold(f64::MAX, f64::min);
        assert!(low > 2.0, "lows {low}");
        assert!(dip < -1.5, "low-mid dip {dip}");
        assert!(total_db(Style::Pultec, &k, 5000.0).abs() < 0.5);
    }

    #[test]
    fn api_q_narrows_as_gain_grows() {
        // Width at half gain, in octaves, for a small and a big mid boost.
        let width = |g: f32| {
            let mut k = Style::Api.defaults();
            k[api::MID_GAIN] = g;
            let half = f64::from(g) / 2.0;
            let mut lo = 1500.0;
            while total_db(Style::Api, &k, lo) > half {
                lo /= 1.01;
            }
            let mut hi = 1500.0;
            while total_db(Style::Api, &k, hi) > half {
                hi *= 1.01;
            }
            (hi / lo).log2()
        };
        let small = width(3.0);
        let big = width(12.0);
        assert!(big < small * 0.7, "small {small:.2} oct, big {big:.2} oct");
    }

    #[test]
    fn maag_air_lifts_the_top_gently() {
        let mut k = Style::Maag.defaults();
        k[maag::AIR_GAIN] = 10.0;
        let at = |f| total_db(Style::Maag, &k, f);
        assert!(at(16_000.0) > 2.0, "16k {}", at(16_000.0));
        assert!(at(1000.0) < 0.5);
        // No peak: monotonic rise.
        assert!(at(8000.0) < at(12_000.0) && at(12_000.0) < at(18_000.0));
    }
}
