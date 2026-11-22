use egui::epaint::PathStroke;
use egui::{Painter, Pos2, Rect, Shape, Stroke, Visuals, pos2};

use super::theme;
use crate::model::{GraphEdge, GraphRow, UNCOMMITTED};

pub const LANE: f32 = 16.0;
const LINE: f32 = 2.0;
const RADIUS: f32 = 4.0;
const STUB: f32 = 0.2;
const DASH: f32 = 3.0;

pub fn width(lanes: u16) -> f32 {
    (lanes as f32 + 1.0) * LANE + LANE / 2.0
}

pub fn paint_row(
    painter: &Painter,
    rect: Rect,
    above: Option<&GraphRow>,
    row: &GraphRow,
    index: usize,
    links: bool,
    visuals: &Visuals,
) {
    let pitch = rect.height();
    let mid = rect.center().y;
    let origin = mid - index as f32 * pitch + RADIUS + LINE / 2.0;
    let x = |lane: u16| rect.left() + LANE * (lane as f32 + 1.0);
    let edge = |e: &GraphEdge, y0: f32| (pos2(x(e.from), y0), pos2(x(e.to), y0 + pitch), e.color);
    let edges: Vec<(Pos2, Pos2, u16)> = above
        .into_iter()
        .flat_map(|a| a.edges.iter().map(|e| edge(e, mid - pitch)))
        .chain(row.edges.iter().map(|e| edge(e, mid)))
        .filter(|_| links)
        .collect();

    for &(a, b, color) in &edges {
        let stroke = Stroke::new(LINE, theme::graph(visuals, color));
        let mut points = segment(a, b, pitch);
        if color == UNCOMMITTED {
            let phase = (a.y - origin).rem_euclid(2.0 * DASH);
            points.insert(0, pos2(a.x, a.y - phase));
            painter.extend(Shape::dashed_line(&points, stroke, DASH, DASH));
        } else {
            painter.add(Shape::line(points, PathStroke::from(stroke)));
        }
    }

    let center = pos2(x(row.lane), mid);
    let color = theme::graph(visuals, row.color);
    if row.head || row.color == UNCOMMITTED {
        painter.circle(center, RADIUS, visuals.panel_fill, Stroke::new(LINE, color));
    } else {
        painter.circle_filled(center, RADIUS, color);
    }
}

fn segment(a: Pos2, b: Pos2, pitch: f32) -> Vec<Pos2> {
    if a.x == b.x {
        return vec![a, b];
    }
    let stub = pitch * STUB;
    let corners = [a, pos2(a.x, a.y + stub), pos2(b.x, b.y - stub), b];
    rounded(&corners, f32::from(theme::radius::L))
}

fn rounded(corners: &[Pos2], radius: f32) -> Vec<Pos2> {
    const STEPS: usize = 6;
    let mut points = vec![corners[0]];
    for w in corners.windows(3) {
        let (prev, corner, next) = (w[0], w[1], w[2]);
        let (into, out) = (corner - prev, next - corner);
        let r = radius.min(into.length() / 2.0).min(out.length() / 2.0);
        let start = corner - into.normalized() * r;
        let end = corner + out.normalized() * r;
        points.extend((0..=STEPS).map(|i| {
            let t = i as f32 / STEPS as f32;
            let u = 1.0 - t;
            pos2(
                u * u * start.x + 2.0 * u * t * corner.x + t * t * end.x,
                u * u * start.y + 2.0 * u * t * corner.y + t * t * end.y,
            )
        }));
    }
    points.push(corners[corners.len() - 1]);
    points
}
