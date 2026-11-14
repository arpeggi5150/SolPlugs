//! The frequency chart every view draws on: log frequency across, dB up, with region labels.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2};
use solplugs_ref_core::bands::{BANDS, REGIONS, centre};
use solplugs_ref_core::group::Span;

use super::theme::Palette;

const LO_HZ: f64 = 20.0;
const HI_HZ: f64 = 20_000.0;

/// Maps bands and dB to screen positions.
pub struct Chart {
    pub rect: Rect,
    pub lo_db: f64,
    pub hi_db: f64,
    /// Display tilt, dB per octave around 1 kHz (a flat line then reads as "balanced").
    pub tilt: f64,
}

impl Chart {
    /// A chart fitted to the given curves (finite values only), at least `min_span` dB tall.
    #[must_use]
    pub fn fit(rect: Rect, tilt: f64, curves: &[&[f64]], min_span: f64) -> Self {
        let mut c = Self {
            rect,
            lo_db: -30.0,
            hi_db: 0.0,
            tilt,
        };
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for curve in curves {
            for (b, &v) in curve.iter().enumerate().take(BANDS) {
                if v.is_finite() && v > -110.0 {
                    let t = c.tilted(b, v);
                    lo = lo.min(t);
                    hi = hi.max(t);
                }
            }
        }
        if lo.is_finite() && hi.is_finite() {
            let mid = (lo + hi) / 2.0;
            let half = ((hi - lo) / 2.0 + 3.0).max(min_span / 2.0);
            c.lo_db = ((mid - half) / 6.0).floor() * 6.0;
            c.hi_db = ((mid + half) / 6.0).ceil() * 6.0;
        }
        c
    }

    /// The displayed value of band `b`.
    #[must_use]
    pub fn tilted(&self, b: usize, v: f64) -> f64 {
        v + self.tilt * (centre(b) / 1000.0).log2()
    }

    #[must_use]
    pub fn x_hz(&self, hz: f64) -> f32 {
        let t = (hz.max(LO_HZ) / LO_HZ).log10() / (HI_HZ / LO_HZ).log10();
        self.rect.left() + t as f32 * self.rect.width()
    }

    #[must_use]
    pub fn x(&self, b: usize) -> f32 {
        self.x_hz(centre(b))
    }

    #[must_use]
    pub fn y(&self, b: usize, v: f64) -> f32 {
        let t = (self.tilted(b, v) - self.lo_db) / (self.hi_db - self.lo_db).max(1e-9);
        self.rect.bottom() - t.clamp(-0.05, 1.05) as f32 * self.rect.height()
    }

    /// Hz at a screen x.
    #[must_use]
    pub fn hz_at(&self, x: f32) -> f64 {
        let t = f64::from((x - self.rect.left()) / self.rect.width().max(1.0));
        LO_HZ * (HI_HZ / LO_HZ).powf(t.clamp(0.0, 1.0))
    }

    /// Background, grid, frequency labels and region names.
    pub fn grid(&self, p: &Painter, pal: &Palette) {
        p.rect_filled(self.rect, 6.0, pal.panel);
        let font = FontId::proportional(10.0);
        for (i, r) in REGIONS.iter().enumerate() {
            let (x0, x1) = (self.x_hz(r.lo_hz), self.x_hz(r.hi_hz));
            if i % 2 == 1 {
                p.rect_filled(
                    Rect::from_x_y_ranges(x0..=x1, self.rect.y_range()),
                    0.0,
                    pal.panel_hi.gamma_multiply(0.5),
                );
            }
            p.text(
                pos2((x0 + x1) / 2.0, self.rect.top() + 4.0),
                Align2::CENTER_TOP,
                r.name,
                font.clone(),
                pal.text_dim.gamma_multiply(0.8),
            );
        }
        for hz in [50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0] {
            let x = self.x_hz(hz);
            p.line_segment(
                [pos2(x, self.rect.top() + 18.0), pos2(x, self.rect.bottom())],
                Stroke::new(1.0, pal.grid),
            );
            let label = if hz >= 1000.0 {
                format!("{}k", hz / 1000.0)
            } else {
                format!("{hz}")
            };
            p.text(
                pos2(x + 2.0, self.rect.bottom() - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                font.clone(),
                pal.text_dim,
            );
        }
        let mut db = (self.lo_db / 6.0).ceil() * 6.0;
        while db < self.hi_db {
            let t = (db - self.lo_db) / (self.hi_db - self.lo_db);
            let y = self.rect.bottom() - t as f32 * self.rect.height();
            p.line_segment(
                [pos2(self.rect.left(), y), pos2(self.rect.right(), y)],
                Stroke::new(1.0, pal.grid.gamma_multiply(0.7)),
            );
            p.text(
                pos2(self.rect.left() + 3.0, y - 1.0),
                Align2::LEFT_BOTTOM,
                format!("{db:.0}"),
                font.clone(),
                pal.text_dim.gamma_multiply(0.7),
            );
            db += 6.0;
        }
    }

    fn points(&self, values: &[f64]) -> Vec<Option<Pos2>> {
        (0..BANDS.min(values.len()))
            .map(|b| {
                (values[b].is_finite() && values[b] > -110.0)
                    .then(|| pos2(self.x(b), self.y(b, values[b])))
            })
            .collect()
    }

    /// A curve; gaps where values are missing.
    pub fn line(&self, p: &Painter, values: &[f64], stroke: Stroke) {
        let mut run: Vec<Pos2> = Vec::new();
        for pt in self.points(values) {
            match pt {
                Some(pt) => run.push(pt),
                None => {
                    if run.len() > 1 {
                        p.add(Shape::line(std::mem::take(&mut run), stroke));
                    }
                    run.clear();
                }
            }
        }
        if run.len() > 1 {
            p.add(Shape::line(run, stroke));
        }
    }

    /// A dashed curve.
    pub fn dashed(&self, p: &Painter, values: &[f64], stroke: Stroke) {
        let pts: Vec<Pos2> = self.points(values).into_iter().flatten().collect();
        if pts.len() > 1 {
            p.extend(Shape::dashed_line(&pts, stroke, 6.0, 4.0));
        }
    }

    /// A curve coloured segment by segment.
    pub fn line_coloured(
        &self,
        p: &Painter,
        values: &[f64],
        width: f32,
        colour: impl Fn(usize) -> Color32,
    ) {
        let pts = self.points(values);
        for b in 1..pts.len() {
            if let (Some(a), Some(c)) = (pts[b - 1], pts[b]) {
                p.line_segment([a, c], Stroke::new(width, colour(b)));
            }
        }
    }

    /// Fills between two curves, band by band, with a colour per band.
    pub fn fill_between(
        &self,
        p: &Painter,
        lo: &[f64],
        hi: &[f64],
        colour: impl Fn(usize) -> Option<Color32>,
    ) {
        let mut mesh = Mesh::default();
        let n = BANDS.min(lo.len()).min(hi.len());
        for b in 1..n {
            let (Some(c0), Some(c1)) = (colour(b - 1), colour(b)) else {
                continue;
            };
            if ![lo[b - 1], lo[b], hi[b - 1], hi[b]]
                .iter()
                .all(|v| v.is_finite() && *v > -110.0)
            {
                continue;
            }
            let i = mesh.vertices.len() as u32;
            let (x0, x1) = (self.x(b - 1), self.x(b));
            for (pos, col) in [
                (pos2(x0, self.y(b - 1, lo[b - 1])), c0),
                (pos2(x0, self.y(b - 1, hi[b - 1])), c0),
                (pos2(x1, self.y(b, hi[b])), c1),
                (pos2(x1, self.y(b, lo[b])), c1),
            ] {
                mesh.vertices.push(Vertex {
                    pos,
                    uv: WHITE_UV,
                    color: col,
                });
            }
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i, i + 2, i + 3);
        }
        if !mesh.vertices.is_empty() {
            p.add(Shape::mesh(mesh));
        }
    }

    /// Shades a group's range.
    pub fn span_band(&self, p: &Painter, spans: &[Option<Span>], colour: Color32) {
        let lo: Vec<f64> = spans.iter().map(|s| s.map_or(f64::NAN, |s| s.lo)).collect();
        let hi: Vec<f64> = spans.iter().map(|s| s.map_or(f64::NAN, |s| s.hi)).collect();
        self.fill_between(p, &lo, &hi, |_| Some(colour));
    }

    /// Highlights a frequency range.
    pub fn highlight(&self, p: &Painter, lo_hz: f64, hi_hz: f64, colour: Color32) {
        p.rect_filled(
            Rect::from_x_y_ranges(self.x_hz(lo_hz)..=self.x_hz(hi_hz), self.rect.y_range()),
            0.0,
            colour,
        );
    }
}

/// Means of spans, as a curve.
#[must_use]
pub fn means(spans: &[Option<Span>]) -> Vec<f64> {
    spans
        .iter()
        .map(|s| s.map_or(f64::NAN, |s| s.mean))
        .collect()
}
