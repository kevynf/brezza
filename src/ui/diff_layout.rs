use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Align, Color32, FontId, TextFormat};

use super::theme;
use crate::model::{DiffLine, LineKind, Syntax};

#[derive(PartialEq)]
pub(super) struct Base {
    pub(super) ptr: usize,
    pub(super) len: usize,
    pub(super) hash: u64,
    pub(super) wrap: bool,
    pub(super) font: FontId,
    pub(super) ppp: f32,
}

pub(super) struct Layout {
    pub(super) base: Base,
    pub(super) limit: u32,
    pub(super) char_w: f32,
    pub(super) digits: f32,
    pub(super) widths: HashMap<char, f32>,
    pub(super) widest: f32,
    pub(super) tops: Option<Arc<Vec<usize>>>,
}

impl Layout {
    pub(super) fn top(&self, i: usize) -> usize {
        match &self.tops {
            Some(tops) => tops[i],
            None => i,
        }
    }
}

pub(super) fn measure(ui: &mut egui::Ui, lines: &[DiffLine], font: &FontId) -> (f32, f32) {
    let char_w = ui
        .painter()
        .layout_no_wrap("0".to_owned(), font.clone(), Color32::TRANSPARENT)
        .size()
        .x;
    let digits = lines
        .iter()
        .flat_map(|l| [l.old_no, l.new_no])
        .flatten()
        .max()
        .unwrap_or(1)
        .to_string()
        .len()
        .max(3) as f32;
    (char_w, digits)
}

pub(super) fn build(
    ui: &mut egui::Ui,
    lines: &[DiffLine],
    wrap: bool,
    limit: f32,
    font: &FontId,
    base: Base,
    (char_w, digits): (f32, f32),
) -> Layout {
    let mut widths = HashMap::new();
    ui.fonts_mut(|fonts| {
        for c in lines.iter().flat_map(|l| code_text(l).chars()) {
            if !c.is_ascii() {
                widths
                    .entry(c)
                    .or_insert_with(|| fonts.glyph_width(font, c));
            }
        }
    });
    let advance = |c: char| if c.is_ascii() { char_w } else { widths[&c] };
    let mut widest = 0.0f32;
    let tops = if wrap {
        let mut tops = Vec::with_capacity(lines.len() + 1);
        tops.push(0usize);
        for line in lines {
            let rows = row_starts(code_text(line), limit, advance).len();
            tops.push(tops[tops.len() - 1] + rows);
        }
        Some(Arc::new(tops))
    } else {
        for line in lines {
            widest = widest.max(code_text(line).chars().map(advance).sum());
        }
        None
    };
    Layout {
        base,
        limit: limit.to_bits(),
        char_w,
        digits,
        widths,
        widest,
        tops,
    }
}

pub(super) fn line_hash(lines: &[DiffLine]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for line in lines {
        let bytes = line.text.as_bytes();
        hash = mix(hash, bytes.len() as u64);
        if let (Some(&first), Some(&middle), Some(&last)) =
            (bytes.first(), bytes.get(bytes.len() / 2), bytes.last())
        {
            hash = mix(hash, u64::from(first));
            hash = mix(hash, u64::from(middle));
            hash = mix(hash, u64::from(last));
        }
        hash = mix(hash, line.kind as u64);
        hash = mix(hash, line.old_no.map_or(u64::MAX, u64::from));
        hash = mix(hash, line.new_no.map_or(u64::MAX, u64::from));
    }
    hash
}

pub(super) fn mix(hash: u64, value: u64) -> u64 {
    (hash ^ value).wrapping_mul(0x0100_0000_01b3)
}

pub(super) fn code_text(line: &DiffLine) -> &str {
    line.text.trim_end_matches('\r')
}

pub(super) fn row_starts(text: &str, limit: f32, mut width: impl FnMut(char) -> f32) -> Vec<usize> {
    let mut starts = vec![0];
    let mut used = 0.0;
    for (i, c) in text.char_indices() {
        let w = width(c);
        if used > 0.0 && used + w > limit {
            starts.push(i);
            used = 0.0;
        }
        used += w;
    }
    starts
}

#[derive(Clone)]
pub(super) struct TextMetrics {
    pub(super) font: FontId,
    pub(super) char_w: f32,
    pub(super) both: bool,
    pub(super) old_right: f32,
    pub(super) new_right: f32,
    pub(super) sign_left: f32,
    pub(super) gutter: f32,
    pub(super) row: f32,
    pub(super) text_height: f32,
}

impl TextMetrics {
    pub(super) fn new(ui: &egui::Ui, digits: usize, both: bool) -> Self {
        use theme::space;
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        let char_w = ui
            .painter()
            .layout_no_wrap("0".to_owned(), font.clone(), Color32::TRANSPARENT)
            .size()
            .x;
        let text_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
        let old_right = space::L + char_w * digits as f32;
        let new_right = if both {
            old_right + space::M + char_w * digits as f32
        } else {
            old_right
        };
        let sign_left = new_right + space::M;
        let gutter = sign_left + char_w + space::M;
        Self {
            font,
            char_w,
            both,
            old_right,
            new_right,
            sign_left,
            gutter,
            row: theme::size::LINE.max(text_height),
            text_height,
        }
    }
}

pub(super) fn metric_digits(max: u32) -> usize {
    max.to_string().len().max(3)
}

pub(super) fn hash_str(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash = mix(hash, u64::from(byte));
    }
    hash
}

pub(super) fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    if text.is_empty() {
        return starts;
    }
    starts.push(0);
    for (i, byte) in text.bytes().enumerate() {
        if byte == b'\n' && i + 1 < text.len() {
            starts.push(i + 1);
        }
    }
    starts
}

pub(super) fn buffer_line<'a>(text: &'a str, starts: &[usize], index: usize) -> &'a str {
    let Some(&start) = starts.get(index) else {
        return "";
    };
    let mut end = starts.get(index + 1).map_or(text.len(), |next| next - 1);
    if end == text.len() && text.ends_with('\n') {
        end -= 1;
    }
    &text[start..end]
}

pub(super) fn block_range(text: &str, starts: &[usize], start: usize, rows: usize) -> Range<usize> {
    let total = starts.len();
    if total == 0 || rows == 0 {
        return 0..0;
    }
    let start = start.min(total - 1);
    let end_line = (start + rows).min(total);
    let mut end = if end_line < total {
        starts[end_line] - 1
    } else {
        text.len()
    };
    if end_line == total && text[..end].ends_with('\n') {
        end -= 1;
    }
    starts[start]..end.max(starts[start])
}

pub(super) fn char_widths<'a>(
    ui: &egui::Ui,
    font: &FontId,
    texts: impl Iterator<Item = &'a str>,
) -> HashMap<char, f32> {
    let mut widths = HashMap::new();
    ui.fonts_mut(|fonts| {
        for c in texts.flat_map(str::chars) {
            if !c.is_ascii() {
                widths
                    .entry(c)
                    .or_insert_with(|| fonts.glyph_width(font, c));
            }
        }
    });
    widths
}

pub(super) fn line_width(line: &str, metrics: &TextMetrics, widths: &HashMap<char, f32>) -> f32 {
    line.chars()
        .map(|c| {
            if c.is_ascii() {
                metrics.char_w
            } else {
                widths.get(&c).copied().unwrap_or(metrics.char_w)
            }
        })
        .sum()
}

pub(super) struct BlockSpans {
    pub(super) syntax: Vec<(Range<usize>, Syntax)>,
    pub(super) emphasis: Vec<(Range<usize>, LineKind)>,
}

pub(super) fn block_job(
    text: &str,
    spans: &BlockSpans,
    visuals: &egui::Visuals,
    font: &FontId,
    line_height: f32,
) -> LayoutJob {
    let p = theme::palette(visuals);
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    job.wrap.break_anywhere = true;
    let mut cuts: Vec<usize> = spans
        .syntax
        .iter()
        .map(|(range, _)| range)
        .chain(spans.emphasis.iter().map(|(range, _)| range))
        .flat_map(|range| [range.start, range.end])
        .chain([0, text.len()])
        .filter(|&cut| cut <= text.len() && text.is_char_boundary(cut))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    let mut format = |start: usize, end: usize| {
        let color = spans
            .syntax
            .iter()
            .rev()
            .find(|(range, _)| range.contains(&start))
            .map_or(p.text, |(_, syntax)| theme::syntax(visuals, *syntax));
        job.append(
            &text[start..end],
            0.0,
            TextFormat {
                font_id: font.clone(),
                color,
                background: Color32::TRANSPARENT,
                line_height: Some(line_height),
                valign: Align::Center,
                ..Default::default()
            },
        );
    };
    for pair in cuts.windows(2) {
        format(pair[0], pair[1]);
    }
    job
}

pub(super) fn line_job(
    line: &DiffLine,
    visuals: &egui::Visuals,
    font: &FontId,
    line_height: f32,
) -> LayoutJob {
    let text = code_text(line);
    let spans = BlockSpans {
        syntax: line
            .syntax
            .iter()
            .filter(|(range, _)| range.end <= text.len())
            .cloned()
            .collect(),
        emphasis: line
            .emphasis
            .iter()
            .filter(|range| range.end <= text.len())
            .map(|range| (range.clone(), line.kind))
            .collect(),
    };
    block_job(text, &spans, visuals, font, line_height)
}

pub(super) fn block_spans(
    text: &str,
    starts: &[usize],
    start_line: usize,
    block_start: usize,
    entries: &[BlockEntry<'_>],
) -> BlockSpans {
    let mut spans = BlockSpans {
        syntax: Vec::new(),
        emphasis: Vec::new(),
    };
    for (k, entry) in entries.iter().enumerate() {
        let Some(line) = entry.line else {
            continue;
        };
        let Some(&line_start) = starts.get(start_line + k) else {
            break;
        };
        let content = buffer_line(text, starts, start_line + k);
        if content != code_text(line) {
            continue;
        }
        let base = line_start - block_start;
        for (range, syntax) in &line.syntax {
            if range.end <= content.len()
                && content.is_char_boundary(range.start)
                && content.is_char_boundary(range.end)
            {
                spans
                    .syntax
                    .push((base + range.start..base + range.end, *syntax));
            }
        }
        for range in &line.emphasis {
            if range.end <= content.len()
                && content.is_char_boundary(range.start)
                && content.is_char_boundary(range.end)
            {
                spans
                    .emphasis
                    .push((base + range.start..base + range.end, line.kind));
            }
        }
    }
    spans
}

pub(super) struct BlockEntry<'a> {
    pub(super) line: Option<&'a DiffLine>,
    pub(super) new_no: u32,
    pub(super) old_no: Option<u32>,
    pub(super) kind: LineKind,
}

#[derive(Clone, Copy)]
pub(super) struct WrapMetrics<'a> {
    pub(super) limit: Option<f32>,
    pub(super) widths: &'a HashMap<char, f32>,
    pub(super) char_w: f32,
}

impl WrapMetrics<'_> {
    pub(super) fn rows(self, text: &str) -> usize {
        let Some(limit) = self.limit else {
            return 1;
        };
        let mut rows = 1;
        let mut used = 0.0;
        for c in text.chars() {
            let width = if c == '\t' {
                self.char_w * 4.0
            } else if c.is_ascii() {
                self.char_w
            } else {
                self.widths.get(&c).copied().unwrap_or(self.char_w)
            };
            if used > 0.0 && used + width > limit {
                rows += 1;
                used = 0.0;
            }
            used += width;
        }
        rows
    }

    pub(super) fn height(self, text: &str, row: f32) -> f32 {
        self.rows(text) as f32 * row
    }
}
