use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{CharIndex, LayoutJob};
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Galley, Margin, Rect, Sense, TextBuffer,
    TextFormat, pos2, vec2,
};

use super::diff_layout::{
    Base, BlockEntry, BlockSpans, Layout, TextMetrics, WrapMetrics, block_job, block_range,
    block_spans, buffer_line, build, char_widths, code_text, hash_str, line_hash, line_job,
    line_starts, line_width, measure, metric_digits, row_starts,
};
use super::diff_plan::{
    SplitMetricsCache, SplitPlan, SplitPlanCache, SplitSource, block_offset, cached_split_plan,
    left_row_height, split_blocks, split_hash, split_row_tops, static_split_metrics,
};
use super::theme;
use crate::model::{DiffLine, Hunk, LineKind};

pub fn file_id(path: &str) -> egui::Id {
    egui::Id::NULL.with(("diff-lines", path))
}

fn split_plan_id(path: &str) -> egui::Id {
    file_id(path).with("split-plan")
}

fn split_metrics_id(path: &str) -> egui::Id {
    file_id(path).with("split-metrics")
}

pub fn forget(ctx: &egui::Context, path: &str) {
    ctx.data_mut(|data| {
        data.remove::<Arc<Layout>>(file_id(path));
        data.remove::<Arc<SplitPlanCache>>(split_plan_id(path));
        data.remove::<Arc<SplitMetricsCache>>(split_metrics_id(path));
    });
}

pub fn lines_view(
    ui: &mut egui::Ui,
    lines: &[DiffLine],
    jump: Option<usize>,
    wrap: bool,
    id: egui::Id,
) {
    use theme::space;
    let visuals = ui.visuals().clone();
    let p = theme::palette(&visuals);
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    let text_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let row = theme::size::LINE.max(text_height);
    let baseline = (row - text_height) / 2.0;
    let base = Base {
        ptr: lines.as_ptr() as usize,
        len: lines.len(),
        hash: line_hash(lines),
        wrap,
        font: font.clone(),
        ppp: ui.ctx().pixels_per_point(),
    };
    let cached = ui.ctx().data_mut(|d| d.get_temp::<Arc<Layout>>(id));
    let (char_w, digits) = match cached.as_ref().filter(|l| l.base == base) {
        Some(layout) => (layout.char_w, layout.digits),
        None => measure(ui, lines, &font),
    };
    let old_right = space::L + char_w * digits;
    let new_right = old_right + space::M + char_w * digits;
    let sign_left = new_right + space::M;
    let gutter = sign_left + char_w + space::M;
    let available = ui.available_width();
    let fixed = !wrap;
    let gutter_metrics = fixed.then(|| TextMetrics::new(ui, digits as usize, true));
    let mut gutter_rows = Vec::new();
    let limit = if wrap {
        (available - gutter - space::M).max(char_w)
    } else {
        f32::INFINITY
    };
    let layout = match cached {
        Some(cached) if cached.base == base && cached.limit == limit.to_bits() => cached,
        _ => {
            let layout = Arc::new(build(ui, lines, wrap, limit, &font, base, (char_w, digits)));
            ui.ctx().data_mut(|d| d.insert_temp(id, layout.clone()));
            layout
        }
    };
    let width = if wrap {
        available
    } else {
        (gutter + layout.widest + space::XL).max(available)
    };
    let (rect, _) = ui.allocate_exact_size(
        vec2(width, row * layout.top(lines.len()) as f32),
        Sense::hover(),
    );
    let y = |visual: usize| rect.top() + row * visual as f32;
    let band = |from: usize, to: usize| {
        Rect::from_min_max(
            pos2(rect.left(), y(layout.top(from))),
            pos2(rect.right(), y(layout.top(to))),
        )
    };
    if let Some(i) = jump {
        let mut target = band(i, i + 1);
        target.set_right(rect.left() + gutter);
        ui.scroll_to_rect(target, Some(Align::Center));
    }
    let clip = ui.clip_rect();
    if !clip.intersects(rect) {
        return;
    }
    let top_row = ((clip.top() - rect.top()) / row).floor().max(0.0) as usize;
    let bottom_row = ((clip.bottom() - rect.top()) / row).ceil().max(0.0) as usize;
    let (first, last) = match &layout.tops {
        Some(tops) => (
            tops.partition_point(|&t| t <= top_row).saturating_sub(1),
            tops.partition_point(|&t| t < bottom_row).min(lines.len()),
        ),
        None => (top_row.min(lines.len()), bottom_row.min(lines.len())),
    };
    let painter = ui.painter();

    let mut i = first;
    while i < last {
        let kind = lines[i].kind;
        let end = (i..last).find(|&j| lines[j].kind != kind).unwrap_or(last);
        if let Some(color) = fill(&visuals, kind, false) {
            let area = band(i, end);
            painter.rect_filled(area, 0.0, color);
            let mut numbers = area;
            numbers.set_right(rect.left() + gutter - space::S);
            painter.rect_filled(numbers, 0.0, color);
        }
        i = end;
    }

    let advance = |c: char| {
        if c.is_ascii() {
            char_w
        } else {
            layout.widths.get(&c).copied().unwrap_or(char_w)
        }
    };
    for (i, line) in lines.iter().enumerate().take(last).skip(first) {
        let top = y(layout.top(i)) + baseline;
        let x = |offset: f32| rect.left() + offset;
        if fixed {
            gutter_rows.push(GutterRow {
                top: y(layout.top(i)),
                height: row,
                kind: line.kind,
                old_no: line.old_no,
                new_no: line.new_no,
            });
        } else {
            let number = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
            let (sign, sign_color, number_color) = match line.kind {
                LineKind::Context => ("", p.text_faint, p.text_faint),
                LineKind::Added => ("+", p.added_text, p.text_muted),
                LineKind::Removed => ("-", p.removed_text, p.text_muted),
            };
            for (right, n) in [(old_right, line.old_no), (new_right, line.new_no)] {
                painter.text(
                    pos2(x(right), top),
                    Align2::RIGHT_TOP,
                    number(n),
                    font.clone(),
                    number_color,
                );
            }
            painter.text(
                pos2(x(sign_left), top),
                Align2::LEFT_TOP,
                sign,
                font.clone(),
                sign_color,
            );
        }
        let text = code_text(line);
        let starts = if wrap {
            row_starts(text, limit, advance)
        } else {
            vec![0]
        };
        let emphasis = fill(&visuals, line.kind, true).unwrap_or(Color32::TRANSPARENT);
        for (k, &start) in starts.iter().enumerate() {
            let end = starts.get(k + 1).copied().unwrap_or(text.len());
            let galley = painter.layout_job(code_job(line, start..end, &visuals, &font));
            let origin = pos2(x(gutter), top + row * k as f32);
            let band_top = y(layout.top(i) + k);
            let offset = |byte: usize| {
                let index = text[start..byte].chars().count();
                galley
                    .rows
                    .first()
                    .map_or(0.0, |r| r.row.x_offset(CharIndex(index)))
            };
            for r in &line.emphasis {
                let (from, to) = (r.start.max(start), r.end.min(end));
                if from >= to || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
                    continue;
                }
                let area = Rect::from_min_max(
                    pos2(origin.x + offset(from), band_top),
                    pos2(origin.x + offset(to), band_top + row),
                );
                let round = |cut: bool| if cut { 0 } else { theme::radius::M };
                let corners = CornerRadius {
                    nw: round(from > r.start),
                    sw: round(from > r.start),
                    ne: round(to < r.end),
                    se: round(to < r.end),
                };
                painter.rect_filled(area, corners, emphasis);
            }
            painter.galley(origin, galley, p.text);
        }
    }
    if let Some(metrics) = gutter_metrics {
        paint_scroll_gutter(ui, &gutter_rows, &metrics, &visuals);
    }
}

fn fill(visuals: &egui::Visuals, kind: LineKind, emphasis: bool) -> Option<Color32> {
    match kind {
        LineKind::Context => None,
        LineKind::Added => Some(theme::added_fill(visuals, emphasis)),
        LineKind::Removed => Some(theme::removed_fill(visuals, emphasis)),
    }
}

fn code_job(
    line: &DiffLine,
    range: std::ops::Range<usize>,
    visuals: &egui::Visuals,
    font: &FontId,
) -> LayoutJob {
    let text_color = theme::palette(visuals).text;
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let text = code_text(line);
    let mut cuts: Vec<usize> = line
        .syntax
        .iter()
        .map(|(r, _)| r)
        .chain(&line.emphasis)
        .flat_map(|r| [r.start, r.end])
        .map(|c| c.clamp(range.start, range.end))
        .chain([range.start, range.end])
        .filter(|&c| text.is_char_boundary(c))
        .collect();
    cuts.sort_unstable();
    cuts.dedup();
    for w in cuts.windows(2) {
        let (start, end) = (w[0], w[1]);
        let color = line
            .syntax
            .iter()
            .rev()
            .find(|(r, _)| r.contains(&start))
            .map_or(text_color, |(_, s)| theme::syntax(visuals, *s));
        job.append(
            &text[start..end],
            0.0,
            TextFormat::simple(font.clone(), color),
        );
    }
    job
}

fn emphasis_shapes(
    visuals: &egui::Visuals,
    galley: &Galley,
    origin: egui::Pos2,
    ranges: &[(Range<usize>, LineKind)],
    clip: Rect,
) -> Vec<egui::Shape> {
    if ranges.is_empty() {
        return Vec::new();
    }
    let text = galley.job.text.as_str();
    let mut shapes = Vec::new();
    let mut row_start = 0usize;
    let mut range_index = 0usize;
    for placed in &galley.rows {
        let mut row_end = row_start;
        for glyph in &placed.row.glyphs {
            row_end += glyph.chr.len_utf8();
        }
        let next = row_end + usize::from(placed.ends_with_newline);
        while range_index < ranges.len() && ranges[range_index].0.end <= row_start {
            range_index += 1;
        }
        let top = origin.y + placed.pos.y;
        let height = placed.row.size.y;
        if top + height >= clip.top() && top <= clip.bottom() {
            let mut index = range_index;
            while index < ranges.len() && ranges[index].0.start < row_end {
                let (range, kind) = &ranges[index];
                index += 1;
                let Some(color) = fill(visuals, *kind, true) else {
                    continue;
                };
                let from = range.start.max(row_start);
                let to = range.end.min(row_end);
                if from >= to || !text.is_char_boundary(from) || !text.is_char_boundary(to) {
                    continue;
                }
                let from_chars = text[row_start..from].chars().count();
                let to_chars = text[row_start..to].chars().count();
                let x0 = placed.pos.x + placed.row.x_offset(CharIndex(from_chars));
                let x1 = placed.pos.x + placed.row.x_offset(CharIndex(to_chars));
                let area =
                    Rect::from_min_max(pos2(origin.x + x0, top), pos2(origin.x + x1, top + height));
                let round = |cut: bool| if cut { 0 } else { theme::radius::M };
                let corners = CornerRadius {
                    nw: round(from > range.start),
                    sw: round(from > range.start),
                    ne: round(to < range.end),
                    se: round(to < range.end),
                };
                shapes.push(egui::Shape::rect_filled(area, corners, color));
            }
        }
        row_start = next;
    }
    shapes
}

fn paint_emphasis_ranges(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    galley: &Galley,
    origin: egui::Pos2,
    ranges: &[(Range<usize>, LineKind)],
) {
    for shape in emphasis_shapes(visuals, galley, origin, ranges, painter.clip_rect()) {
        painter.add(shape);
    }
}

fn paint_band(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    kind: LineKind,
) {
    if let Some(color) = fill(visuals, kind, false) {
        painter.rect_filled(
            Rect::from_min_size(pos2(left, top), vec2(width, height)),
            0.0,
            color,
        );
    }
}

#[expect(clippy::too_many_arguments)]
fn paint_numbers(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    metrics: &TextMetrics,
    left: f32,
    top: f32,
    height: f32,
    kind: LineKind,
    old_no: Option<u32>,
    new_no: Option<u32>,
) {
    let p = theme::palette(visuals);
    let (sign, sign_color) = match kind {
        LineKind::Context => ("", p.text_faint),
        LineKind::Added => ("+", p.added_text),
        LineKind::Removed => ("-", p.removed_text),
    };
    let number_color = match kind {
        LineKind::Context => p.text_faint,
        LineKind::Added | LineKind::Removed => p.text_muted,
    };
    let baseline = top + ((height - metrics.text_height) * 0.5).max(0.0);
    if metrics.both {
        painter.text(
            pos2(left + metrics.old_right, baseline),
            Align2::RIGHT_TOP,
            old_no.map(|no| no.to_string()).unwrap_or_default(),
            metrics.font.clone(),
            number_color,
        );
    }
    painter.text(
        pos2(left + metrics.new_right, baseline),
        Align2::RIGHT_TOP,
        new_no.map(|no| no.to_string()).unwrap_or_default(),
        metrics.font.clone(),
        number_color,
    );
    painter.text(
        pos2(left + metrics.sign_left, baseline),
        Align2::LEFT_TOP,
        sign,
        metrics.font.clone(),
        sign_color,
    );
}

#[derive(Clone, Copy)]
struct GutterRow {
    top: f32,
    height: f32,
    kind: LineKind,
    old_no: Option<u32>,
    new_no: Option<u32>,
}

fn paint_scroll_gutter(
    ui: &egui::Ui,
    rows: &[GutterRow],
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
) {
    let clip = ui.clip_rect();
    let gutter = Rect::from_min_size(clip.left_top(), vec2(metrics.gutter, clip.height()));
    let painter = ui.painter().with_clip_rect(gutter);
    painter.rect_filled(gutter, 0.0, theme::palette(visuals).panel);
    for row in rows {
        if row.top + row.height < gutter.top() || row.top > gutter.bottom() {
            continue;
        }
        if let Some(color) = fill(visuals, row.kind, false) {
            let area = Rect::from_min_max(
                pos2(gutter.left(), row.top.max(gutter.top())),
                pos2(gutter.right(), (row.top + row.height).min(gutter.bottom())),
            );
            painter.rect_filled(area, 0.0, color);
        }
        paint_numbers(
            &painter,
            visuals,
            metrics,
            gutter.left(),
            row.top,
            row.height,
            row.kind,
            row.old_no,
            row.new_no,
        );
    }
}

fn row_galley(
    ui: &egui::Ui,
    text: &str,
    spans: &BlockSpans,
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
    wrap: Option<f32>,
) -> Arc<egui::Galley> {
    let mut job = block_job(text, spans, visuals, &metrics.font, metrics.row);
    job.wrap.max_width = wrap.unwrap_or(f32::INFINITY);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn walk_rows(galley: &egui::Galley, top: f32, mut add: impl FnMut(usize, bool, f32, f32)) {
    let mut line = 0usize;
    let mut first = true;
    for row in &galley.rows {
        add(line, first, top + row.pos.y, row.row.size.y);
        if row.ends_with_newline {
            line += 1;
            first = true;
        } else {
            first = false;
        }
    }
}

fn line_count(text: &str) -> usize {
    if text.is_empty() {
        0
    } else {
        text.bytes().filter(|&byte| byte == b'\n').count() + 1
    }
}

struct SliceBuffer<'a> {
    text: &'a mut String,
    start: usize,
    end: usize,
}

impl SliceBuffer<'_> {
    fn byte(&self, index: CharIndex) -> usize {
        self.as_str()
            .char_indices()
            .nth(index.0)
            .map_or(self.as_str().len(), |(byte, _)| byte)
    }
}

impl TextBuffer for SliceBuffer<'_> {
    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<SliceBuffer<'static>>()
    }

    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        &self.text[self.start..self.end]
    }

    fn insert_text(&mut self, text: &str, char_index: CharIndex) -> usize {
        let offset = self.byte(char_index);
        self.text.insert_str(self.start + offset, text);
        self.end += text.len();
        text.chars().count()
    }

    fn delete_char_range(&mut self, char_range: Range<CharIndex>) {
        let start = self.byte(char_range.start);
        let end = self.byte(char_range.end);
        self.text
            .replace_range(self.start + start..self.start + end, "");
        self.end -= end - start;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockStyle {
    Unified,
    UnifiedScrolled,
    SplitRight,
    SplitTail,
}

impl BlockStyle {
    fn numbers(self) -> bool {
        !matches!(self, Self::UnifiedScrolled | Self::SplitRight)
    }

    fn both_numbers(self) -> bool {
        matches!(self, Self::Unified | Self::UnifiedScrolled)
    }
}

struct EditableOutput {
    height: f32,
    delta: Option<isize>,
}

#[expect(clippy::disallowed_methods, clippy::too_many_arguments)]
fn editable_block(
    ui: &mut egui::Ui,
    id: egui::Id,
    text: &mut String,
    starts: &[usize],
    start_line: usize,
    entries: &[BlockEntry<'_>],
    width: f32,
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
    wrap: bool,
    style: BlockStyle,
    mut gutter: Option<&mut Vec<GutterRow>>,
) -> EditableOutput {
    let rows = entries.len().max(1);
    let range = block_range(text, starts, start_line, entries.len());
    let spans = block_spans(text, starts, start_line, range.start, entries);
    let text_width = (width - metrics.gutter).max(metrics.char_w);
    let block_id = id.with(("block", entries.first().map_or(0, |e| e.new_no)));
    let block_hash = hash_str(&text[range.clone()]);
    let galley = row_galley(
        ui,
        &text[range.clone()],
        &spans,
        metrics,
        visuals,
        wrap.then_some(text_width),
    );
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let clip = ui.clip_rect();
    let height = galley.rect.height().max(metrics.row);
    let area = Rect::from_min_size(pos2(left, top), vec2(width, height));
    if !ui.memory(|memory| memory.has_focus(block_id)) && !clip.intersects(area) {
        ui.add_space(height);
        return EditableOutput {
            height,
            delta: None,
        };
    }
    walk_rows(&galley, top, |line, first, y, height| {
        if y + height < clip.top() || y > clip.bottom() {
            return;
        }
        let entry = entries.get(line);
        let kind = entry.map_or(LineKind::Context, |entry| entry.kind);
        paint_band(ui.painter(), visuals, left, y, width, height, kind);
        if first {
            let (old, new) = match entry {
                Some(entry) => match style {
                    BlockStyle::Unified | BlockStyle::UnifiedScrolled => {
                        (entry.old_no, Some(entry.new_no))
                    }
                    BlockStyle::SplitRight | BlockStyle::SplitTail => (None, Some(entry.new_no)),
                },
                None => (None, None),
            };
            if let Some(gutter) = &mut gutter {
                gutter.push(GutterRow {
                    top: y,
                    height,
                    kind,
                    old_no: old,
                    new_no: new,
                });
            }
            if style.numbers() {
                let number_height = if style.both_numbers() {
                    height
                } else {
                    metrics.row
                };
                paint_numbers(
                    ui.painter(),
                    visuals,
                    metrics,
                    left,
                    y,
                    number_height,
                    kind,
                    old,
                    new,
                );
            }
        }
    });
    if galley.rows.is_empty() {
        paint_band(
            ui.painter(),
            visuals,
            left,
            top,
            width,
            metrics.row,
            LineKind::Context,
        );
        let number = match style {
            BlockStyle::Unified | BlockStyle::UnifiedScrolled => Some(1),
            BlockStyle::SplitRight | BlockStyle::SplitTail => {
                entries.first().map(|entry| entry.new_no)
            }
        };
        if let Some(gutter) = &mut gutter {
            gutter.push(GutterRow {
                top,
                height: metrics.row,
                kind: LineKind::Context,
                old_no: None,
                new_no: number,
            });
        }
        if style.numbers() {
            paint_numbers(
                ui.painter(),
                visuals,
                metrics,
                left,
                top,
                metrics.row,
                LineKind::Context,
                None,
                number,
            );
        }
    }
    let emphasis = ui.painter().add(egui::Shape::Noop);
    let font = metrics.font.clone();
    let visuals = visuals.clone();
    let mut layouter = |ui: &egui::Ui, buffer: &dyn TextBuffer, wrap_width: f32| {
        if hash_str(buffer.as_str()) == block_hash {
            return galley.clone();
        }
        let mut job = block_job(buffer.as_str(), &spans, &visuals, &font, metrics.row);
        job.wrap.max_width = if wrap { wrap_width } else { f32::INFINITY };
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut slice = SliceBuffer {
        text,
        start: range.start,
        end: range.end,
    };
    let output = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(metrics.gutter);
        egui::TextEdit::multiline(&mut slice)
            .id(block_id)
            .font(font.clone())
            .layouter(&mut layouter)
            .desired_width(text_width)
            .desired_rows(rows)
            .min_size(vec2(text_width, metrics.row))
            .frame(egui::Frame::NONE)
            .margin(Margin::ZERO)
            .lock_focus(true)
            .show(ui)
    });
    let output = output.inner;
    let new_rows = output
        .response
        .changed()
        .then(|| line_count(slice.as_str()));
    let new_starts = line_starts(text);
    let new_range = block_range(text, &new_starts, start_line, entries.len());
    let new_spans = block_spans(text, &new_starts, start_line, new_range.start, entries);
    let shapes = emphasis_shapes(
        &visuals,
        &output.galley,
        output.galley_pos,
        &new_spans.emphasis,
        ui.clip_rect(),
    );
    ui.painter().set(emphasis, egui::Shape::Vec(shapes));
    let delta = new_rows.map(|new_rows| new_rows as isize - rows as isize);
    EditableOutput {
        height: output.galley.size().y.max(metrics.row),
        delta,
    }
}

#[expect(clippy::too_many_arguments)]
pub fn unified_edit(
    ui: &mut egui::Ui,
    text: &mut String,
    hunks: &[crate::model::Hunk],
    jump: Option<usize>,
    wrap: bool,
    width: f32,
    id: egui::Id,
    sizes: &mut HashMap<u32, usize>,
) -> bool {
    let visuals = ui.visuals().clone();
    let plan: Vec<&DiffLine> = hunks.iter().flat_map(|hunk| &hunk.lines).collect();
    let mut max_no = plan
        .iter()
        .flat_map(|line| [line.old_no, line.new_no])
        .flatten()
        .max()
        .unwrap_or(1);
    max_no = max_no.max(text.lines().count() as u32);
    let metrics = TextMetrics::new(ui, metric_digits(max_no), true);
    let content_width = if wrap {
        width
    } else {
        let removed: Vec<&str> = plan
            .iter()
            .filter(|line| line.new_no.is_none())
            .map(|line| code_text(line))
            .collect();
        let widths = char_widths(
            ui,
            &metrics.font,
            std::iter::once(text.as_str()).chain(removed.iter().copied()),
        );
        let widest = std::iter::once(text.as_str())
            .chain(removed.iter().copied())
            .flat_map(|text| text.split('\n'))
            .map(|line| line_width(line, &metrics, &widths))
            .fold(0.0, f32::max);
        width.max(metrics.gutter + widest + theme::space::XL)
    };
    let text_width = (content_width - metrics.gutter).max(metrics.char_w);
    let mut changed = false;
    let fixed = !wrap;
    let mut gutter_rows = Vec::new();
    let style = if fixed {
        BlockStyle::UnifiedScrolled
    } else {
        BlockStyle::Unified
    };
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_min_width(content_width);
        let mut starts = line_starts(text);
        let mut shift: isize = 0;
        let mut covered = 0usize;
        let mut target = None;
        let mut index = 0;
        while index < plan.len() {
            if plan[index].new_no.is_some() {
                let first_no = plan[index].new_no.unwrap();
                let begin = index;
                while index < plan.len() && plan[index].new_no.is_some() {
                    index += 1;
                }
                let plan_rows = index - begin;
                let wanted = sizes.get(&first_no).copied().unwrap_or(plan_rows);
                let start = (first_no as isize - 1 + shift).max(0) as usize;
                let row_count = wanted.min(starts.len().saturating_sub(start));
                if row_count == 0 {
                    shift -= plan_rows as isize;
                    continue;
                }
                let planned = plan_rows.min(row_count);
                let mut entries: Vec<BlockEntry<'_>> = plan[begin..begin + planned]
                    .iter()
                    .map(|line| BlockEntry {
                        line: Some(*line),
                        new_no: line.new_no.unwrap(),
                        old_no: line.old_no,
                        kind: line.kind,
                    })
                    .collect();
                for k in planned..row_count {
                    entries.push(BlockEntry {
                        line: None,
                        new_no: first_no + k as u32,
                        old_no: None,
                        kind: LineKind::Context,
                    });
                }
                let top = ui.cursor().top();
                let left = ui.cursor().left();
                if let Some(jump) = jump
                    && (begin..index).contains(&jump)
                {
                    target = Some(Rect::from_min_size(
                        pos2(left, top + (jump - begin) as f32 * metrics.row),
                        vec2(metrics.gutter, metrics.row),
                    ));
                }
                let mut step = 0;
                let output = editable_block(
                    ui,
                    id,
                    text,
                    &starts,
                    start,
                    &entries,
                    content_width,
                    &metrics,
                    &visuals,
                    wrap,
                    style,
                    fixed.then_some(&mut gutter_rows),
                );
                if let Some(delta) = output.delta {
                    changed = true;
                    step = delta;
                    starts = line_starts(text);
                }
                let actual = (row_count as isize + step).max(0) as usize;
                if step != 0 {
                    sizes.insert(first_no, actual);
                }
                shift += actual as isize - plan_rows as isize;
                covered = covered.max(start + actual);
            } else {
                let begin = index;
                while index < plan.len() && plan[index].new_no.is_none() {
                    index += 1;
                }
                let top = ui.cursor().top();
                let left = ui.cursor().left();
                let clip = ui.clip_rect();
                let painter = ui.painter();
                let text_color = theme::palette(&visuals).text;
                let mut y = top;
                for (k, line) in plan[begin..index].iter().enumerate() {
                    let mut job = line_job(line, &visuals, &metrics.font, metrics.row);
                    job.wrap.max_width = if wrap { text_width } else { f32::INFINITY };
                    let galley = painter.layout_job(job);
                    let height = galley.rect.height();
                    if let Some(jump) = jump
                        && begin + k == jump
                    {
                        target = Some(Rect::from_min_size(
                            pos2(left, y),
                            vec2(metrics.gutter, height),
                        ));
                    }
                    if y + height >= clip.top() && y <= clip.bottom() {
                        walk_rows(&galley, y, |_, first, row_y, row_height| {
                            if row_y + row_height < clip.top() || row_y > clip.bottom() {
                                return;
                            }
                            paint_band(
                                painter,
                                &visuals,
                                left,
                                row_y,
                                content_width,
                                row_height,
                                LineKind::Removed,
                            );
                            if first {
                                if fixed {
                                    gutter_rows.push(GutterRow {
                                        top: row_y,
                                        height: row_height,
                                        kind: LineKind::Removed,
                                        old_no: line.old_no,
                                        new_no: None,
                                    });
                                } else {
                                    paint_numbers(
                                        painter,
                                        &visuals,
                                        &metrics,
                                        left,
                                        row_y,
                                        row_height,
                                        LineKind::Removed,
                                        line.old_no,
                                        None,
                                    );
                                }
                            }
                        });
                        let ranges: Vec<_> = line
                            .emphasis
                            .iter()
                            .filter(|range| range.end <= line.text.len())
                            .cloned()
                            .map(|range| (range, line.kind))
                            .collect();
                        paint_emphasis_ranges(
                            painter,
                            &visuals,
                            &galley,
                            pos2(left + metrics.gutter, y),
                            &ranges,
                        );
                        painter.galley(pos2(left + metrics.gutter, y), galley, text_color);
                    }
                    y += height;
                }
                ui.add_space(y - top);
            }
        }
        let total = starts.len();
        if covered < total || total == 0 {
            let begin = covered.min(total);
            let rows = if total == 0 {
                1
            } else {
                total.saturating_sub(begin)
            };
            let mut entries = Vec::with_capacity(rows);
            for k in 0..rows {
                let line = if total == 0 { 0 } else { begin + k } as u32 + 1;
                entries.push(BlockEntry {
                    line: None,
                    new_no: line,
                    old_no: None,
                    kind: LineKind::Context,
                });
            }
            let start_line = if total == 0 { 0 } else { begin };
            if editable_block(
                ui,
                id,
                text,
                &starts,
                start_line,
                &entries,
                content_width,
                &metrics,
                &visuals,
                wrap,
                style,
                fixed.then_some(&mut gutter_rows),
            )
            .delta
            .is_some()
            {
                changed = true;
            }
        }
        if let Some(target) = target {
            ui.scroll_to_rect(target, Some(Align::Center));
        }
    });
    if fixed {
        paint_scroll_gutter(ui, &gutter_rows, &metrics, &visuals);
    }
    changed
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SplitSide {
    Left,
    Right,
}

#[expect(clippy::too_many_arguments)]
fn paint_split_line(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    metrics: &TextMetrics,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    line: &DiffLine,
    side: SplitSide,
    wrap: Option<f32>,
) {
    let kind = match (side, line.kind) {
        (SplitSide::Left, LineKind::Removed) => LineKind::Removed,
        (SplitSide::Right, LineKind::Added) => LineKind::Added,
        _ => LineKind::Context,
    };
    if let Some(color) = fill(visuals, kind, false) {
        painter.rect_filled(
            Rect::from_min_size(pos2(left, top), vec2(width, height)),
            0.0,
            color,
        );
    }
    let mut job = line_job(line, visuals, &metrics.font, metrics.row);
    job.wrap.max_width = wrap.unwrap_or(f32::INFINITY);
    let galley = painter.layout_job(job);
    let ranges: Vec<_> = line
        .emphasis
        .iter()
        .filter(|range| range.end <= line.text.len())
        .cloned()
        .map(|range| (range, line.kind))
        .collect();
    paint_emphasis_ranges(
        painter,
        visuals,
        &galley,
        pos2(left + metrics.gutter, top),
        &ranges,
    );
    painter.galley(
        pos2(left + metrics.gutter, top),
        galley,
        theme::palette(visuals).text,
    );
}

#[expect(clippy::too_many_arguments)]
fn paint_split_rows(
    ui: &mut egui::Ui,
    plan: &SplitPlan,
    lines: &[&DiffLine],
    side: SplitSide,
    rows: Range<usize>,
    heights: &[f32],
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
    content: f32,
    wrap: Option<f32>,
) {
    let clip = ui.clip_rect();
    let left = ui.cursor().left();
    let mut y = ui.cursor().top();
    let mut pending = 0.0f32;
    for row in rows {
        let height = heights.get(row).copied().unwrap_or(0.0);
        let top = y;
        y += height;
        if top >= clip.bottom() || y <= clip.top() {
            pending += height;
            continue;
        }
        if pending > 0.0 {
            ui.add_space(pending);
            pending = 0.0;
        }
        let line = match side {
            SplitSide::Left => plan.rows[row].left,
            SplitSide::Right => plan.rows[row].right,
        };
        if let Some(line) = line.and_then(|line| lines.get(line)) {
            paint_split_line(
                ui.painter(),
                visuals,
                metrics,
                left,
                top,
                content,
                height,
                line,
                side,
                wrap,
            );
        }
        ui.add_space(height);
    }
    if pending > 0.0 {
        ui.add_space(pending);
    }
}

#[expect(clippy::too_many_arguments)]
fn paint_fixed_gutter(
    ui: &egui::Ui,
    plan: &SplitPlan,
    lines: &[&DiffLine],
    side: SplitSide,
    rect: Rect,
    viewport_height: f32,
    offset: f32,
    row_tops: &[f32],
    heights: &[f32],
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
    tail_editor: bool,
) {
    if side == SplitSide::Right && tail_editor {
        return;
    }
    let p = theme::palette(visuals);
    let gutter = Rect::from_min_size(rect.min, vec2(metrics.gutter, viewport_height));
    let painter = ui.painter().with_clip_rect(gutter);
    painter.rect_filled(gutter, 0.0, p.panel);
    let bottom = rect.top() + viewport_height;
    for (row, row_ref) in plan.rows.iter().enumerate() {
        let top = rect.top() + row_tops.get(row).copied().unwrap_or(0.0) - offset;
        let height = heights.get(row).copied().unwrap_or(0.0);
        if top + height < rect.top() || top > bottom {
            continue;
        }
        let line = match side {
            SplitSide::Left => row_ref.left,
            SplitSide::Right => row_ref.right,
        }
        .and_then(|line| lines.get(line));
        let Some(line) = line else {
            continue;
        };
        let kind = match (side, line.kind) {
            (SplitSide::Left, LineKind::Removed) => LineKind::Removed,
            (SplitSide::Right, LineKind::Added) => LineKind::Added,
            _ => LineKind::Context,
        };
        if let Some(color) = fill(visuals, kind, false) {
            let area = Rect::from_min_max(
                pos2(rect.left(), top.max(rect.top())),
                pos2(rect.left() + metrics.gutter, (top + height).min(bottom)),
            );
            painter.rect_filled(area, 0.0, color);
        }
        let number = match side {
            SplitSide::Left => line.old_no,
            SplitSide::Right => line.new_no,
        };
        if let Some(number) = number {
            paint_numbers(
                &painter,
                visuals,
                metrics,
                rect.left(),
                top,
                height,
                kind,
                None,
                Some(number),
            );
        }
    }
}

pub struct SplitEdit<'a> {
    pub text: &'a mut String,
    pub sizes: &'a mut HashMap<u32, usize>,
}

#[expect(clippy::too_many_arguments)]
fn split_right_edit(
    ui: &mut egui::Ui,
    plan: &SplitPlan,
    lines: &[&DiffLine],
    blocks: &[Range<usize>],
    base_id: egui::Id,
    text: &mut String,
    sizes: &mut HashMap<u32, usize>,
    metrics: &TextMetrics,
    visuals: &egui::Visuals,
    content: f32,
    wrap: bool,
    widths: &HashMap<char, f32>,
    char_w: f32,
    row_heights: &mut [f32],
    block_heights: &mut [f32],
) -> bool {
    let wrap_metrics = WrapMetrics {
        limit: wrap.then_some((content - metrics.gutter).max(metrics.char_w)),
        widths,
        char_w,
    };
    let mut starts = line_starts(text);
    let mut shift = 0isize;
    let mut changed = false;
    for (block_index, block) in blocks.iter().enumerate() {
        let Some(first_row) = plan.rows.get(block.start) else {
            continue;
        };
        if first_row.right.is_none() {
            let mut total = 0.0;
            for row in block.clone() {
                let height = left_row_height(plan.rows[row], lines, metrics, wrap_metrics);
                row_heights[row] = height;
                total += height;
            }
            block_heights[block_index] = total;
            ui.add_space(total);
            continue;
        }
        let Some(first_line) = first_row.right.and_then(|line| lines.get(line)) else {
            continue;
        };
        let Some(first_no) = first_line.new_no else {
            continue;
        };
        let plan_rows = block.len();
        let wanted = sizes.get(&first_no).copied().unwrap_or(plan_rows);
        let start = (first_no as isize - 1 + shift).max(0) as usize;
        let empty = text.is_empty() && start == 0;
        let available = if empty {
            1
        } else {
            starts.len().saturating_sub(start)
        };
        let row_count = wanted.min(available).max(if empty { 1 } else { 0 });
        let left_totals: Vec<f32> = block
            .clone()
            .map(|row| left_row_height(plan.rows[row], lines, metrics, wrap_metrics))
            .collect();
        if row_count == 0 {
            let total: f32 = left_totals.iter().sum();
            for (offset, &height) in left_totals.iter().enumerate() {
                row_heights[block.start + offset] = height;
            }
            block_heights[block_index] = total;
            ui.add_space(total);
            shift -= plan_rows as isize;
            continue;
        }
        let planned = plan_rows.min(row_count);
        let mut entries = Vec::with_capacity(row_count);
        for &row in &plan.rows[block.start..block.start + planned] {
            let Some(line) = row.right.and_then(|line| lines.get(line)) else {
                continue;
            };
            let Some(new_no) = line.new_no else {
                continue;
            };
            entries.push(BlockEntry {
                line: Some(line),
                new_no,
                old_no: line.old_no,
                kind: line.kind,
            });
        }
        for offset in planned..row_count {
            entries.push(BlockEntry {
                line: None,
                new_no: first_no + offset as u32,
                old_no: None,
                kind: LineKind::Context,
            });
        }
        let output = editable_block(
            ui,
            base_id,
            text,
            &starts,
            start,
            &entries,
            content,
            metrics,
            visuals,
            wrap,
            BlockStyle::SplitRight,
            None,
        );
        if output.delta.is_some() {
            changed = true;
            starts = line_starts(text);
        }
        let mut actual_rows = (row_count as isize + output.delta.unwrap_or(0)).max(0) as usize;
        if actual_rows == 0 && text.is_empty() && start == 0 {
            actual_rows = 1;
        }
        let mut left_sum = 0.0;
        for (offset, &left) in left_totals.iter().enumerate() {
            let right = if offset < actual_rows && start + offset < starts.len() {
                wrap_metrics.height(buffer_line(text, &starts, start + offset), metrics.row)
            } else {
                0.0
            };
            let height = left.max(right);
            row_heights[block.start + offset] = height;
            left_sum += height;
        }
        let target = left_sum.max(output.height);
        block_heights[block_index] = target;
        if target > output.height {
            ui.add_space(target - output.height);
        }
        let delta = output.delta.unwrap_or(0);
        if delta != 0 || empty {
            sizes.insert(first_no, actual_rows);
        }
        shift += actual_rows as isize - plan_rows as isize;
    }
    changed
}

#[derive(Clone, Copy)]
struct SplitScroll {
    offset: egui::Vec2,
}

impl Default for SplitScroll {
    fn default() -> Self {
        Self {
            offset: egui::Vec2::ZERO,
        }
    }
}

pub fn split_view(
    ui: &mut egui::Ui,
    path: &str,
    hunks: &[Hunk],
    jump: Option<usize>,
    wrap: bool,
    mut edit: Option<SplitEdit<'_>>,
) -> bool {
    let lines: Vec<&DiffLine> = hunks.iter().flat_map(|hunk| &hunk.lines).collect();
    let text_lines = edit.as_ref().map_or(0, |edit| edit.text.lines().count());
    let source = SplitSource {
        ptr: hunks.as_ptr() as usize,
        len: hunks.len(),
        lines: lines.len(),
        hash: split_hash(&lines),
    };
    let plan = cached_split_plan(ui.ctx(), split_plan_id(path), source, &lines);
    let max_no = plan.max_no.max(text_lines.min(u32::MAX as usize) as u32);
    let metrics = TextMetrics::new(ui, metric_digits(max_no), false);
    let visuals = ui.visuals().clone();
    let base_id = file_id(path).with("split");
    let size = ui.available_size();
    let gap = theme::space::M;
    let pane = ((size.x - gap) * 0.5).max(theme::size::CONTROL * 4.0);
    let pane_height = size.y.max(theme::size::LINE * 4.0);
    let wrap_width = wrap.then_some((pane - metrics.gutter).max(metrics.char_w));
    let editing = edit.is_some();
    let mut widest = 0.0f32;
    let edit_widths = if let Some(edit) = &edit {
        let mut texts: Vec<&str> = lines.iter().map(|line| code_text(line)).collect();
        texts.extend(edit.text.lines());
        let widths = char_widths(ui, &metrics.font, texts.iter().copied());
        if !wrap {
            for text in &texts {
                widest = widest.max(line_width(text, &metrics, &widths));
            }
        }
        Some(widths)
    } else {
        None
    };
    let cached_metrics = if editing {
        None
    } else {
        Some(static_split_metrics(
            ui,
            split_metrics_id(path),
            source,
            &plan,
            &lines,
            &metrics,
            wrap,
            wrap_width,
        ))
    };
    if let Some(cache) = &cached_metrics {
        widest = cache.widest;
    }
    let content = if wrap {
        pane
    } else {
        pane.max(metrics.gutter + widest + theme::space::XL)
    };
    let static_heights: &[f32] = cached_metrics
        .as_ref()
        .map_or(&[], |cache| cache.row_heights.as_slice());
    let blocks = if editing {
        split_blocks(&plan)
    } else {
        Vec::new()
    };
    let tail_editor = editing && plan.right_runs.is_empty();
    let mut tail_height = 0.0f32;
    let mut edit_row_heights = if editing {
        vec![0.0f32; plan.rows.len()]
    } else {
        Vec::new()
    };
    let mut block_heights = if editing {
        vec![0.0f32; blocks.len()]
    } else {
        Vec::new()
    };
    let scroll_id = file_id(path).with("split-scroll");
    let shared = ui
        .ctx()
        .data_mut(|data| data.get_temp::<SplitScroll>(scroll_id).unwrap_or_default());
    let mut changed = false;
    let (_, outer) = ui.allocate_space(vec2(size.x, pane_height));
    let left_rect = Rect::from_min_size(outer.min, vec2(pane, pane_height));
    let right_rect = Rect::from_min_size(
        pos2(outer.left() + pane + gap, outer.top()),
        vec2(pane, pane_height),
    );
    let right_viewport;
    let right_offset;
    {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right_rect)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        let out = egui::ScrollArea::both()
            .id_salt(("split-right", path))
            .auto_shrink(false)
            .max_width(pane)
            .max_height(pane_height)
            .vertical_scroll_offset(shared.offset.y)
            .horizontal_scroll_offset(shared.offset.x)
            .show(&mut child, |ui| {
                ui.set_min_width(content);
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(edit) = edit.as_mut() {
                    if tail_editor {
                        let starts = line_starts(edit.text);
                        let rows = starts.len().max(1);
                        let entries: Vec<BlockEntry<'_>> = (0..rows)
                            .map(|offset| BlockEntry {
                                line: None,
                                new_no: offset as u32 + 1,
                                old_no: None,
                                kind: LineKind::Context,
                            })
                            .collect();
                        let output = editable_block(
                            ui,
                            base_id,
                            &mut *edit.text,
                            &starts,
                            0,
                            &entries,
                            content,
                            &metrics,
                            &visuals,
                            wrap,
                            BlockStyle::SplitTail,
                            None,
                        );
                        changed |= output.delta.is_some();
                        tail_height = output.height;
                    }
                    changed |= split_right_edit(
                        ui,
                        &plan,
                        &lines,
                        &blocks,
                        base_id,
                        &mut *edit.text,
                        &mut *edit.sizes,
                        &metrics,
                        &visuals,
                        content,
                        wrap,
                        edit_widths.as_ref().unwrap(),
                        metrics.char_w,
                        &mut edit_row_heights,
                        &mut block_heights,
                    );
                } else {
                    paint_split_rows(
                        ui,
                        &plan,
                        &lines,
                        SplitSide::Right,
                        0..plan.rows.len(),
                        static_heights,
                        &metrics,
                        &visuals,
                        content,
                        wrap_width,
                    );
                }
            });
        right_viewport = out.inner_rect.size();
        right_offset = out.state.clamped_offset();
    }
    let left_offset;
    {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left_rect)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        let out = egui::ScrollArea::both()
            .id_salt(("split-left", path))
            .auto_shrink(false)
            .max_width(pane)
            .max_height(pane_height)
            .vertical_scroll_offset(shared.offset.y)
            .horizontal_scroll_offset(shared.offset.x)
            .show(&mut child, |ui| {
                ui.set_min_width(content);
                ui.spacing_mut().item_spacing.y = 0.0;
                if editing {
                    for (block, &target) in blocks.iter().zip(&block_heights) {
                        paint_split_rows(
                            ui,
                            &plan,
                            &lines,
                            SplitSide::Left,
                            block.clone(),
                            &edit_row_heights,
                            &metrics,
                            &visuals,
                            content,
                            wrap_width,
                        );
                        let used: f32 = edit_row_heights[block.clone()].iter().sum();
                        if target > used {
                            ui.add_space(target - used);
                        }
                    }
                    if tail_editor {
                        ui.add_space(tail_height);
                    }
                } else {
                    paint_split_rows(
                        ui,
                        &plan,
                        &lines,
                        SplitSide::Left,
                        0..plan.rows.len(),
                        static_heights,
                        &metrics,
                        &visuals,
                        content,
                        wrap_width,
                    );
                }
            });
        left_offset = out.state.clamped_offset();
    }
    let gutter_heights = if editing {
        edit_row_heights.as_slice()
    } else {
        static_heights
    };
    let row_tops = split_row_tops(
        &plan,
        &blocks,
        &block_heights,
        gutter_heights,
        tail_height,
        false,
    );
    paint_fixed_gutter(
        ui,
        &plan,
        &lines,
        SplitSide::Left,
        left_rect,
        right_viewport.y,
        left_offset.y,
        &row_tops,
        gutter_heights,
        &metrics,
        &visuals,
        tail_editor,
    );
    paint_fixed_gutter(
        ui,
        &plan,
        &lines,
        SplitSide::Right,
        right_rect,
        right_viewport.y,
        right_offset.y,
        &row_tops,
        gutter_heights,
        &metrics,
        &visuals,
        tail_editor,
    );
    let content_height: f32 = if editing {
        block_heights.iter().sum::<f32>() + tail_height
    } else {
        cached_metrics
            .as_ref()
            .map_or(0.0, |cache| cache.content_height)
    };
    let mut next = shared;
    if (left_offset - shared.offset).length_sq() > 0.25 {
        next.offset = left_offset;
    }
    if (right_offset - shared.offset).length_sq() > 0.25 {
        next.offset = right_offset;
    }
    if let Some(jump) = jump
        && let Some(&row) = plan.line_rows.get(jump)
    {
        let y = if editing {
            block_offset(&blocks, &block_heights, &edit_row_heights, row)
        } else {
            static_heights[..row.min(static_heights.len())]
                .iter()
                .sum::<f32>()
        };
        next.offset.y = y - right_viewport.y * 0.5;
    }
    let max_y = (content_height - right_viewport.y).max(0.0);
    let max_x = (content - right_viewport.x).max(0.0);
    next.offset.y = next.offset.y.clamp(0.0, max_y);
    next.offset.x = next.offset.x.clamp(0.0, max_x);
    ui.ctx().data_mut(|data| data.insert_temp(scroll_id, next));
    ui.painter().vline(
        outer.left() + pane + gap * 0.5,
        outer.top()..=(outer.top() + pane_height),
        egui::Stroke::new(1.0, theme::palette(&visuals).border),
    );
    changed
}
