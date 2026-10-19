//! `egui_plot` helpers for rendering price series.
//!
//! Targets egui_plot 0.29, where the axis bounds are configured on the [`Plot`]
//! builder (via `include_x`/`include_y`), the grid and legend are builder flags,
//! and individual items (the line) are added inside `Plot::show` on the supplied
//! `PlotUi`.

use egui::Color32;
use egui_plot::{Line, Plot};

use crate::models::PricePoint;

/// Render a single-line price chart of the given (already time-range-filtered)
/// points. The X axis is measured in days since the first point and labelled with
/// human-readable dates; the Y axis is the closing price.
pub fn render_line_chart(ui: &mut egui::Ui, points: &[PricePoint], color: Color32, legend: &str) {
    if points.len() < 2 {
        ui.add_space(20.0);
        ui.label(egui::RichText::new("Not enough data in this range to plot yet.").weak());
        return;
    }

    let first = points[0].date;

    // egui_plot 0.29 materialises a series from `[f64; 2]` tuples (there is no
    // `From<Vec<(f64, f64)>>` for `PlotPoints`), so collect pairs while also
    // tracking the axis extents in the same pass.
    let mut series: Vec<[f64; 2]> = Vec::with_capacity(points.len());
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    let mut max_days = 0.0_f64;
    for p in points {
        let x = p.date.signed_duration_since(first).num_days() as f64;
        max_days = max_days.max(x);
        ymin = ymin.min(p.close);
        ymax = ymax.max(p.close);
        series.push([x, p.close]);
    }

    // Pad the range a little so the line is not glued to the borders, and avoid a
    // degenerate (flat) range that egui_plot cannot scale.
    let (ymin, ymax) = if !ymin.is_finite() || !ymax.is_finite() || (ymax - ymin).abs() < 1e-9 {
        let center = ymax.max(1.0).abs();
        (center - center * 0.05 - 0.5, center + center * 0.05 + 0.5)
    } else {
        let span = (ymax - ymin) * 0.05;
        (ymin - span, ymax + span)
    };
    let max_days = max_days.max(1.0);

    Plot::new("price_chart")
        .height(ui.available_height().max(240.0))
        .show_x(true)
        .show_y(true)
        .show_grid(true)
        .legend(egui_plot::Legend::default())
        .x_axis_formatter(move |mark, _range| {
            let d = first + chrono::TimeDelta::days(mark.value as i64);
            d.format("%b %Y").to_string()
        })
        .include_x(0.0)
        .include_x(max_days)
        .include_y(ymin)
        .include_y(ymax)
        .show(ui, |plot| {
            plot.line(
                Line::new(series)
                    .name(legend.to_string())
                    .color(color)
                    .width(1.6_f32),
            );
        });

    if let Some(p) = points.last() {
        ui.label(egui::RichText::new(format!("{}  →  {}", first, p.date)).small().weak());
    }
}

/// One price series to overlay on a comparison chart, each drawn in its own
/// colour with a legend entry.
pub struct Overlay {
    pub name: String,
    pub color: Color32,
    pub points: Vec<PricePoint>,
}

/// A fixed, high-contrast palette used to distinguish compared assets on one
/// chart. `palette_color(index)` cycles through it.
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(70, 160, 230),  // blue
    Color32::from_rgb(230, 120, 60),  // orange
    Color32::from_rgb(120, 200, 120), // green
    Color32::from_rgb(220, 80, 120),  // pink
    Color32::from_rgb(180, 130, 230), // purple
    Color32::from_rgb(230, 200, 80),  // yellow
    Color32::from_rgb(80, 200, 200),  // teal
    Color32::from_rgb(200, 200, 200), // gray
];

/// The colour assigned to the asset at `index` within the comparison set.
pub fn palette_color(index: usize) -> Color32 {
    PALETTE[index % PALETTE.len()]
}

/// Render several price series on one chart, each in its own colour with a
/// legend. When `normalize` is true, every series is expressed as a percentage
/// relative to its first value, so a cheap asset and a pricey one stay readable
/// on a single Y axis.
pub fn render_comparison_chart(ui: &mut egui::Ui, overlays: &[Overlay], normalize: bool) {
    // A common X origin (the earliest date across all series) so every line lines
    // up on the same time axis.
    let origin = overlays
        .iter()
        .filter_map(|o| o.points.first().map(|p| p.date))
        .min();

    // Materialise each series into `[f64; 2]` points while tracking the Y
    // extents and the largest X, exactly like the single-series renderer.
    let mut lines: Vec<Line> = Vec::new();
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    let mut max_days = 0.0_f64;

    for o in overlays {
        if o.points.is_empty() {
            continue;
        }
        let base_date = o.points[0].date;
        let first_close = o.points[0].close;
        let mut pts: Vec<[f64; 2]> = Vec::with_capacity(o.points.len());
        for p in &o.points {
            let x = (p.date - origin.unwrap_or(base_date)).num_days() as f64;
            max_days = max_days.max(x);
            let y = if normalize && first_close.abs() > 1e-9 {
                p.close / first_close * 100.0
            } else {
                p.close
            };
            ymin = ymin.min(y);
            ymax = ymax.max(y);
            pts.push([x, y]);
        }
        if !pts.is_empty() {
            lines.push(
                Line::new(pts)
                    .name(o.name.clone())
                    .color(o.color)
                    .width(1.6_f32),
            );
        }
    }

    if lines.is_empty() {
        ui.add_space(20.0);
        ui.label(egui::RichText::new("Not enough data in this range to plot yet.").weak());
        return;
    }

    // Pad the range and guard against a degenerate (flat / non-finite) span.
    let (ymin, ymax) = if !ymin.is_finite() || !ymax.is_finite() || (ymax - ymin).abs() < 1e-9 {
        let center = ymax.max(1.0).abs();
        (center - center * 0.05 - 0.5, center + center * 0.05 + 0.5)
    } else {
        let span = (ymax - ymin) * 0.05;
        (ymin - span, ymax + span)
    };
    let max_days = max_days.max(1.0);
    let origin = origin.unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap());

    Plot::new("comparison_chart")
        .height(ui.available_height().max(280.0))
        .show_x(true)
        .show_y(true)
        .show_grid(true)
        .legend(egui_plot::Legend::default())
        .x_axis_formatter(move |mark, _range| {
            let d = origin + chrono::TimeDelta::days(mark.value as i64);
            d.format("%b %Y").to_string()
        })
        .include_x(0.0)
        .include_x(max_days)
        .include_y(ymin)
        .include_y(ymax)
        .show(ui, |plot| {
            for line in lines {
                plot.line(line);
            }
        });

    if normalize {
        ui.label(
            egui::RichText::new("Normalized: 100 = value at the start of the range (percentage change).")
                .small()
                .weak(),
        );
    }
}