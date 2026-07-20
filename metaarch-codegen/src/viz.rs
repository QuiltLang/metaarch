//! Generator for `docs/topology.svg`: the architecture diagram — one box per
//! service, an arrow lane per emitted event from its emitter down into every
//! consumer, and a cylinder per database. The same SVG element is embedded
//! inline in `docs/index.html`.
//!
//! Built as plain text, not a `.rs.quilt` metaprogram: every spec-driven value
//! in an SVG lands at *attribute* position (coordinates, sizes, the viewBox),
//! and the quilt html target's covered splice position is text interiors
//! (`raw_text`) — the same only-tested-positions rule the nix generator
//! follows. Layout is pure integer arithmetic over declaration order, so the
//! diagram is deterministic like every other artifact.

use std::fmt::Write as _;

use metaarch_spec::{Lang, Service, SystemSpec};

use crate::header;

const MARGIN: i32 = 20;
const BOX_H: i32 = 46;
const BOX_GAP: i32 = 56;
const LANE_H: i32 = 26;
const DB_RX: i32 = 34;
const DB_BODY_H: i32 = 22;

/// Per-language accent color for a service box's border (rust crab orange,
/// python logo blue) — the only non-neutral ink in the diagram.
fn lang_color(lang: Lang) -> &'static str {
    match lang {
        Lang::Rust => "#b7410e",
        Lang::Python => "#306998",
    }
}

/// Approximate rendered width of `s` in the diagram's 13px sans-serif.
fn text_w(s: &str) -> i32 {
    s.len() as i32 * 8
}

/// The meta line under a service name: `lang · address`.
fn meta_line(service: &Service, index: usize) -> String {
    format!(
        "{} · {}",
        service.lang.expect("validated: service has a lang"),
        crate::service_addr(service, index)
    )
}

/// Box width for a service: wide enough for its name and meta line.
fn box_w(service: &Service, index: usize) -> i32 {
    let meta = meta_line(service, index);
    // The 11px meta line runs ~7px per char.
    (text_w(&service.name).max(meta.len() as i32 * 7) + 24).max(120)
}

/// The standalone `docs/topology.svg` artifact contents.
pub(crate) fn topology_svg_file(spec: &SystemSpec) -> String {
    format!("{} -->\n{}", header(&spec.name, "<!--"), svg(spec))
}

/// The `<svg>` element itself — also spliced into `docs/index.html`.
pub(crate) fn svg(spec: &SystemSpec) -> String {
    // One arrow lane per emitted event, in declaration order.
    let lanes: Vec<(usize, &str)> = spec
        .services
        .iter()
        .enumerate()
        .flat_map(|(i, s)| s.emits.iter().map(move |e| (i, e.name.as_str())))
        .collect();

    let box_y = MARGIN + lanes.len() as i32 * LANE_H + 8;
    let widths: Vec<i32> = spec
        .services
        .iter()
        .enumerate()
        .map(|(i, s)| box_w(s, i))
        .collect();
    let xs: Vec<i32> = widths
        .iter()
        .scan(MARGIN, |x, w| {
            let here = *x;
            *x += w + BOX_GAP;
            Some(here)
        })
        .collect();
    let center = |i: usize| xs[i] + widths[i] / 2;

    let has_db = spec.services.iter().any(|s| s.db.is_some());
    let width = xs.last().copied().unwrap_or(MARGIN)
        + widths.last().copied().unwrap_or(0)
        + MARGIN;
    let height = box_y + BOX_H + if has_db { 96 } else { MARGIN };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" font-family=\"sans-serif\">"
    );
    let _ = writeln!(out, "<title>{} — topology</title>", spec.name);
    out.push_str(
        "<defs><marker id=\"arrow\" markerWidth=\"7\" markerHeight=\"7\" refX=\"5\" refY=\"3\" orient=\"auto\"><path d=\"M0,0 L6,3 L0,6 z\" fill=\"#555\"/></marker></defs>\n",
    );

    // Event lanes: emitter box top → up to the lane → across → down into each
    // consumer box top. Verticals attach at a per-lane x offset so parallel
    // lanes never overlap on a shared box.
    for (lane, &(emitter, event)) in lanes.iter().enumerate() {
        let lane_y = MARGIN + lane as i32 * LANE_H + LANE_H / 2;
        let attach = |i: usize| center(i) + lane as i32 * 14 - (lanes.len() as i32 - 1) * 7;
        let from_x = attach(emitter);
        let consumers: Vec<usize> = spec
            .services
            .iter()
            .enumerate()
            .filter(|(_, s)| s.consumes.iter().any(|(n, _)| n == event))
            .map(|(i, _)| i)
            .collect();
        if consumers.is_empty() {
            // No consumers yet: a stub line makes the emission still visible.
            let _ = writeln!(
                out,
                "<path d=\"M{from_x},{box_y} V{lane_y}\" fill=\"none\" stroke=\"#555\"/>"
            );
        }
        for &consumer in &consumers {
            // A self-consuming service loops back beside its own attach point.
            let to_x = if consumer == emitter {
                from_x + 18
            } else {
                attach(consumer)
            };
            let _ = writeln!(
                out,
                "<path d=\"M{from_x},{box_y} V{lane_y} H{to_x} V{arrow_y}\" fill=\"none\" stroke=\"#555\" marker-end=\"url(#arrow)\"/>",
                arrow_y = box_y - 4,
            );
        }
        let _ = writeln!(
            out,
            "<text x=\"{x}\" y=\"{y}\" font-size=\"11\" fill=\"#555\">{event}</text>",
            x = from_x + 6,
            y = lane_y - 4,
        );
    }

    // Service boxes, then databases hanging under their service.
    for (i, service) in spec.services.iter().enumerate() {
        let lang = service.lang.expect("validated: service has a lang");
        let (x, w, cx) = (xs[i], widths[i], center(i));
        let _ = writeln!(
            out,
            "<rect x=\"{x}\" y=\"{box_y}\" width=\"{w}\" height=\"{BOX_H}\" rx=\"6\" fill=\"#f8f8f8\" stroke=\"{color}\" stroke-width=\"1.5\"/>",
            color = lang_color(lang),
        );
        let _ = writeln!(
            out,
            "<text x=\"{cx}\" y=\"{y}\" font-size=\"13\" font-weight=\"bold\" text-anchor=\"middle\" fill=\"#222\">{name}</text>",
            y = box_y + 19,
            name = service.name,
        );
        let _ = writeln!(
            out,
            "<text x=\"{cx}\" y=\"{y}\" font-size=\"11\" text-anchor=\"middle\" fill=\"#777\">{meta}</text>",
            y = box_y + 36,
            meta = meta_line(service, i),
        );
        if let Some(db) = &service.db {
            let top = box_y + BOX_H + 26;
            let _ = writeln!(
                out,
                "<line x1=\"{cx}\" y1=\"{y1}\" x2=\"{cx}\" y2=\"{top}\" stroke=\"#999\"/>",
                y1 = box_y + BOX_H,
            );
            // Cylinder: body path with a bottom arc, then the top ellipse.
            let _ = writeln!(
                out,
                "<path d=\"M{left},{top} V{bottom} A{DB_RX},7 0 0 0 {right},{bottom} V{top}\" fill=\"#fff\" stroke=\"#999\"/>",
                left = cx - DB_RX,
                right = cx + DB_RX,
                bottom = top + DB_BODY_H,
            );
            let _ = writeln!(
                out,
                "<ellipse cx=\"{cx}\" cy=\"{top}\" rx=\"{DB_RX}\" ry=\"7\" fill=\"#fff\" stroke=\"#999\"/>",
            );
            let _ = writeln!(
                out,
                "<text x=\"{cx}\" y=\"{y}\" font-size=\"11\" text-anchor=\"middle\" fill=\"#777\">{engine} · {n} table(s)</text>",
                y = top + DB_BODY_H + 24,
                engine = crate::sql::engine_name(db.engine),
                n = db.tables.len(),
            );
        }
    }
    out.push_str("</svg>");
    out
}
