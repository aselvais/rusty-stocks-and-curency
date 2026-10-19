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