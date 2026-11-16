//! Shared custom widgets: the dial gauge and a lightweight time-series plot.

use egui::{Color32, Pos2, Stroke, vec2};

/// Responsive column layout: as many columns as fit at `min_item_width`
/// (up to `max_cols`), with each item stretched to fill the available
/// width (up to `max_item_width`). Returns (columns, item width).
pub fn fit_columns(
    avail_width: f32,
    min_item_width: f32,
    max_item_width: f32,
    max_cols: usize,
    spacing: f32,
) -> (usize, f32) {
    let cols = ((avail_width + spacing) / (min_item_width + spacing)).floor() as usize;
    let cols = cols.clamp(1, max_cols);
    let width = ((avail_width - spacing * (cols.saturating_sub(1)) as f32) / cols as f32)
        .min(max_item_width);
    (cols, width)
}

/// Semicircular dial gauge with ticks, needle, and a numeric readout.
/// When `warn` is set the needle and value flash red.
/// Returns the outer rect so callers can overlay small annotations.
#[allow(clippy::too_many_arguments)]
pub fn gauge(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    title: &str,
    value: f64,
    min: f64,
    max: f64,
    decimals: usize,
    warn: bool,
) -> egui::Rect {
    // Draw the border ourselves instead of using Frame::group: a frame adds
    // inner margins around the content, which would make the widget wider
    // than the computed cell size and break the right-edge padding math.
    let (outer, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(outer);
    // Warn state: solid red panel with a bright border, so an out-of-range
    // reading is visible at a glance.
    let (fill, border) = if warn {
        (Color32::from_rgb(160, 30, 30), Color32::from_rgb(255, 60, 60))
    } else {
        (
            ui.visuals().extreme_bg_color,
            ui.style().visuals.widgets.noninteractive.bg_stroke.color,
        )
    };
    painter.rect_filled(outer, 4.0, fill);
    painter.rect_stroke(
        outer,
        4.0,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let rect = outer.shrink(6.0);

    let center = Pos2::new(rect.center().x, rect.top() + rect.height() * 0.72);
    let radius = (rect.width().min(rect.height() * 1.2)) / 2.0 - 16.0;
    // Arc, ticks, labels, and title switch to soft white so they stay
    // readable on the red warn fill.
    let dim = if warn {
        Color32::from_rgba_unmultiplied(255, 255, 255, 190)
    } else {
        ui.visuals().weak_text_color()
    };
    let fg = ui.visuals().text_color();

    // Dial arc (180 degrees, left = min, right = max).
    let arc: Vec<Pos2> = (0..=48)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 48.0);
            center + vec2(radius * a.cos(), -radius * a.sin())
        })
        .collect();
    painter.add(egui::Shape::line(arc, Stroke::new(3.0, dim)));

    // Ticks and labels.
    for i in 0..5 {
        let frac = i as f32 / 4.0;
        let tick_val = min + (max - min) * frac as f64;
        let a = std::f32::consts::PI * (1.0 - frac);
        let dir = vec2(a.cos(), -a.sin());
        painter.line_segment(
            [center + dir * (radius - 8.0), center + dir * radius],
            Stroke::new(2.0, dim),
        );
        let label = if max - min < 25.0 {
            format!("{tick_val:.1}")
        } else {
            format!("{}", tick_val as i32)
        };
        painter.text(
            center + dir * (radius + 11.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(10.0),
            dim,
        );
    }

    // Needle.
    let frac = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
    let a = std::f32::consts::PI * (1.0 - frac);
    let needle_color = if warn {
        Color32::WHITE
    } else {
        Color32::from_rgb(200, 40, 40)
    };
    painter.line_segment(
        [center, center + vec2(a.cos(), -a.sin()) * (radius - 10.0)],
        Stroke::new(2.5, needle_color),
    );
    painter.circle_filled(center, 4.5, Color32::DARK_GRAY);

    // Title and value.
    painter.text(
        Pos2::new(center.x, center.y - radius * 0.62),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(13.0),
        dim,
    );
    let value_color = if warn {
        Color32::WHITE
    } else {
        fg
    };
    painter.text(
        Pos2::new(center.x, center.y - 14.0),
        egui::Align2::CENTER_CENTER,
        format!("{value:.decimals$}"),
        egui::FontId::new(19.0, egui::FontFamily::Proportional),
        value_color,
    );
    outer
}

pub const SERIES_COLORS: [Color32; 9] = [
    Color32::from_rgb(239, 83, 80),   // red
    Color32::from_rgb(66, 165, 245),  // blue
    Color32::from_rgb(255, 167, 38),  // orange
    Color32::from_rgb(102, 187, 106), // green
    Color32::from_rgb(171, 71, 188),  // purple
    Color32::from_rgb(38, 198, 218),  // cyan
    Color32::from_rgb(255, 238, 88),  // yellow
    Color32::from_rgb(236, 64, 122),  // pink
    Color32::from_rgb(141, 110, 99),  // brown
];

pub struct Series<'a> {
    pub color: Color32,
    /// (time_s, value) points, already normalized if desired by the caller.
    pub points: &'a [[f64; 2]],
}

/// Minimal time-series plot: axis box, Y gridlines with labels, one polyline
/// per series, and a vertical playback cursor at `cursor_t`.
pub fn time_plot(
    ui: &mut egui::Ui,
    series: &[Series],
    x_max: f64,
    cursor_t: Option<f64>,
    height: f32,
) {
    let desired = vec2(ui.available_width(), height);
    let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if rect.width() < 60.0 {
        return;
    }
    let painter = ui.painter_at(rect);
    let dim = ui.visuals().weak_text_color();

    // Reserve left margin for Y labels.
    let left = 44.0;
    let plot = egui::Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top() + 4.0),
        Pos2::new(rect.right() - 6.0, rect.bottom() - 18.0),
    );

    // Y range across all series (fall back to 0..1).
    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in series {
        for p in s.points {
            y_min = y_min.min(p[1]);
            y_max = y_max.max(p[1]);
        }
    }
    if !y_min.is_finite() || y_max - y_min < 1e-9 {
        y_min = 0.0;
        y_max = 1.0;
    }
    let pad = (y_max - y_min) * 0.05;
    y_min -= pad;
    y_max += pad;

    let to_screen = |x: f64, y: f64| -> Pos2 {
        let fx = (x / x_max.max(1e-6)) as f32;
        let fy = ((y - y_min) / (y_max - y_min)) as f32;
        Pos2::new(
            plot.left() + fx * plot.width(),
            plot.bottom() - fy * plot.height(),
        )
    };

    // Frame and horizontal gridlines.
    painter.rect_stroke(plot, 0.0, Stroke::new(1.0, dim), egui::StrokeKind::Inside);
    for i in 0..=4 {
        let v = y_min + (y_max - y_min) * (i as f64 / 4.0);
        let y = to_screen(0.0, v).y;
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(0.5, dim.gamma_multiply(0.4)),
        );
        painter.text(
            Pos2::new(plot.left() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            format!("{v:.0}"),
            egui::FontId::proportional(10.0),
            dim,
        );
    }
    // X end labels.
    painter.text(
        Pos2::new(plot.left(), plot.bottom() + 4.0),
        egui::Align2::LEFT_TOP,
        "0 s",
        egui::FontId::proportional(10.0),
        dim,
    );
    painter.text(
        Pos2::new(plot.right(), plot.bottom() + 4.0),
        egui::Align2::RIGHT_TOP,
        format!("{x_max:.0} s"),
        egui::FontId::proportional(10.0),
        dim,
    );

    // Series polylines, downsampled to roughly the pixel width.
    let max_pts = plot.width().max(64.0) as usize * 2;
    painter.with_clip_rect(plot).extend(series.iter().map(|s| {
        let stride = (s.points.len() / max_pts).max(1);
        let pts: Vec<Pos2> = s
            .points
            .iter()
            .step_by(stride)
            .map(|p| to_screen(p[0], p[1]))
            .collect();
        egui::Shape::line(pts, Stroke::new(1.5, s.color))
    }));

    // Playback cursor.
    if let Some(t) = cursor_t {
        let x = to_screen(t, y_min).x;
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.5, Color32::WHITE),
        );
        painter.text(
            Pos2::new(x, plot.top() - 2.0),
            egui::Align2::CENTER_BOTTOM,
            format!("{t:.1} s"),
            egui::FontId::proportional(11.0),
            ui.visuals().text_color(),
        );
    }
}
