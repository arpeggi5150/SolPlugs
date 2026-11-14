//! Drawing kit for frequency graphs: the log frequency axis (every octave the same width),
//! the fine grid with octave lines and labels, dB lines every 3 dB, spectrum traces and
//! EQ curves. Shared by Clean's big graph, the hardware styles' analyzer and the side
//! column's spectrometer.

use egui::epaint::PathShape;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2};

use super::analyzer::{Analyzer, Which};
use super::theme::Palette;

/// Lowest frequency shown.
pub const F_LO: f32 = 20.0;
/// Highest frequency shown.
pub const F_HI: f32 = 20_000.0;

/// Octave lines and their labels (standard octave centres).
pub const OCTAVES: [(f32, &str); 10] = [
    (31.5, "31"),
    (63.0, "63"),
    (125.0, "125"),
    (250.0, "250"),
    (500.0, "500"),
    (1000.0, "1k"),
    (2000.0, "2k"),
    (4000.0, "4k"),
    (8000.0, "8k"),
    (16_000.0, "16k"),
];

/// A frequency/level plot area.
#[derive(Clone, Copy, Debug)]
pub struct Axes {
    /// The plot rect.
    pub rect: Rect,
    /// EQ scale: ±this many dB.
    pub db_range: f32,
    /// Spectrum scale, dBFS: bottom and top.
    pub spec: (f32, f32),
}

impl Axes {
    /// x of `f`.
    pub fn x(&self, f: f32) -> f32 {
        let t = (f.max(1.0) / F_LO).ln() / (F_HI / F_LO).ln();
        self.rect.left() + t * self.rect.width()
    }

    /// Frequency at `x`.
    pub fn f(&self, x: f32) -> f32 {
        let t = (x - self.rect.left()) / self.rect.width().max(1.0);
        F_LO * (F_HI / F_LO).powf(t)
    }

    /// y of an EQ gain.
    pub fn y(&self, db: f32) -> f32 {
        let t = (db / self.db_range).clamp(-1.05, 1.05);
        self.rect.center().y - t * self.rect.height() * 0.5
    }

    /// EQ gain at `y`.
    pub fn db(&self, y: f32) -> f32 {
        (self.rect.center().y - y) / (self.rect.height() * 0.5) * self.db_range
    }

    /// y of a spectrum level.
    pub fn y_spec(&self, dbfs: f32) -> f32 {
        let (lo, hi) = self.spec;
        let t = ((dbfs - lo) / (hi - lo)).clamp(0.0, 1.0);
        self.rect.bottom() - t * self.rect.height()
    }

    /// Frequency ratio one pixel spans.
    pub fn px_ratio(&self) -> f32 {
        (F_HI / F_LO).powf(1.0 / self.rect.width().max(1.0))
    }
}

/// Background grid: third-octave lines, octave lines, dB lines every 3 dB (stronger at
/// 0). Labels: every octave under the plot, dB on the left when `db_labels`.
pub fn grid(p: &Painter, a: &Axes, pal: &Palette, labels: bool, db_labels: bool) {
    let r = a.rect;
    let fine = Stroke::new(1.0, pal.grid.gamma_multiply(0.45));
    let oct = Stroke::new(1.0, pal.grid.gamma_multiply(1.4));
    // Third-octave lines from 20 Hz.
    let mut f = F_LO;
    while f <= F_HI * 1.001 {
        let x = a.x(f);
        p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], fine);
        f *= 2f32.powf(1.0 / 3.0);
    }
    let font = FontId::proportional(if r.width() < 380.0 { 9.5 } else { 11.0 });
    for (f, label) in OCTAVES {
        let x = a.x(f);
        p.line_segment([pos2(x, r.top()), pos2(x, r.bottom())], oct);
        if labels {
            p.text(
                pos2(x, r.bottom() + 3.0),
                Align2::CENTER_TOP,
                label,
                font.clone(),
                pal.dim,
            );
        }
    }
    if labels {
        for (f, label, al) in [(F_LO, "20", Align2::LEFT_TOP), (F_HI, "20k", Align2::RIGHT_TOP)] {
            p.text(
                pos2(a.x(f), r.bottom() + 3.0),
                al,
                label,
                font.clone(),
                pal.dim.gamma_multiply(0.7),
            );
        }
    }
    let step = if a.db_range > 18.0 { 6.0 } else { 3.0 };
    let mut db = -a.db_range;
    while db <= a.db_range + 0.01 {
        let y = a.y(db);
        let s = if db.abs() < 0.01 {
            Stroke::new(1.2, pal.dim.gamma_multiply(0.7))
        } else {
            Stroke::new(1.0, pal.grid.gamma_multiply(0.9))
        };
        p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], s);
        if db_labels {
            p.text(
                pos2(r.left() - 4.0, y),
                Align2::RIGHT_CENTER,
                format!("{db:+.0}"),
                font.clone(),
                pal.dim,
            );
        }
        db += step;
    }
}

/// A spectrum trace: a line, and a fill down to the floor when `fill` is given.
pub fn spectrum(
    p: &Painter,
    a: &Axes,
    an: &Analyzer,
    which: Which,
    line: Color32,
    fill: Option<Color32>,
) {
    let r = a.rect;
    let spread = a.px_ratio().sqrt();
    let w = r.width().max(1.0) as usize;
    let pts: Vec<Pos2> = (0..=w)
        .map(|i| {
            let x = r.left() + i as f32;
            let lvl = an.level(which, a.f(x), spread);
            pos2(x, a.y_spec(lvl))
        })
        .collect();
    if let Some(fill) = fill {
        // Vertical strips: robust for any shape.
        for pt in &pts {
            if pt.y < r.bottom() - 0.5 {
                p.line_segment([*pt, pos2(pt.x, r.bottom())], Stroke::new(1.0, fill));
            }
        }
    }
    p.add(Shape::Path(PathShape::line(pts, Stroke::new(1.3, line))));
}

/// An EQ curve from a gain function (dB at a frequency).
pub fn curve(p: &Painter, a: &Axes, stroke: Stroke, db_at: impl Fn(f32) -> f32) {
    let r = a.rect;
    let w = r.width().max(1.0) as usize;
    let pts: Vec<Pos2> = (0..=w / 2)
        .map(|i| {
            let x = r.left() + (2 * i) as f32;
            pos2(x, a.y(db_at(a.f(x))))
        })
        .collect();
    p.add(Shape::Path(PathShape::line(pts, stroke)));
}

/// A filled EQ curve: shaded between the curve and 0 dB.
pub fn curve_fill(p: &Painter, a: &Axes, colour: Color32, db_at: impl Fn(f32) -> f32) {
    let r = a.rect;
    let zero = a.y(0.0);
    let w = r.width().max(1.0) as usize;
    for i in 0..=w / 2 {
        let x = r.left() + (2 * i) as f32;
        let y = a.y(db_at(a.f(x)));
        if (y - zero).abs() > 0.5 {
            p.line_segment([pos2(x, y), pos2(x, zero)], Stroke::new(2.0, colour));
        }
    }
}

/// Short Hz text for labels ("850", "1.5k").
pub fn hz_short(f: f32) -> String {
    if f >= 10_000.0 {
        format!("{:.0}k", f / 1000.0)
    } else if f >= 1000.0 {
        format!("{:.1}k", f / 1000.0)
    } else {
        format!("{f:.0}")
    }
}
