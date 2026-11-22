use std::ops::Range;
use std::sync::Arc;

use egui::FontId;

use super::diff_layout::{
    TextMetrics, WrapMetrics, char_widths, code_text, line_job, line_width, mix,
};
use crate::model::{DiffLine, LineKind};

#[derive(Clone, Copy)]
pub(super) struct SplitRowRef {
    pub(super) left: Option<usize>,
    pub(super) right: Option<usize>,
}

pub(super) struct SplitPlan {
    pub(super) rows: Vec<SplitRowRef>,
    pub(super) right_runs: Vec<Range<usize>>,
    pub(super) line_rows: Vec<usize>,
    pub(super) max_no: u32,
}

impl SplitPlan {
    pub(super) fn build(lines: &[&DiffLine]) -> Self {
        let mut rows = Vec::with_capacity(lines.len());
        let mut line_rows = vec![0usize; lines.len()];
        let mut max_no = 0u32;
        for line in lines {
            for number in [line.old_no, line.new_no].into_iter().flatten() {
                max_no = max_no.max(number);
            }
        }
        let mut index = 0;
        while index < lines.len() {
            if lines[index].kind == LineKind::Context {
                line_rows[index] = rows.len();
                rows.push(SplitRowRef {
                    left: Some(index),
                    right: Some(index),
                });
                index += 1;
                continue;
            }
            let begin = index;
            while index < lines.len() && lines[index].kind != LineKind::Context {
                index += 1;
            }
            let base = rows.len();
            let mut removed = Vec::new();
            let mut added = Vec::new();
            for (offset, line) in lines[begin..index].iter().enumerate() {
                match line.kind {
                    LineKind::Removed => removed.push(begin + offset),
                    LineKind::Added => added.push(begin + offset),
                    LineKind::Context => {}
                }
            }
            let paired = removed.len().min(added.len());
            for (offset, (&left, &right)) in removed.iter().zip(&added).enumerate() {
                rows.push(SplitRowRef {
                    left: Some(left),
                    right: Some(right),
                });
                line_rows[left] = base + offset;
                line_rows[right] = base + offset;
            }
            for (offset, &line) in removed.iter().enumerate().skip(paired) {
                rows.push(SplitRowRef {
                    left: Some(line),
                    right: None,
                });
                line_rows[line] = base + offset;
            }
            for (offset, &line) in added.iter().enumerate().skip(paired) {
                rows.push(SplitRowRef {
                    left: None,
                    right: Some(line),
                });
                line_rows[line] = base + offset;
            }
        }
        let mut right_runs = Vec::new();
        let mut row = 0usize;
        while row < rows.len() {
            if rows[row].right.is_none() {
                row += 1;
                continue;
            }
            let begin = row;
            while row < rows.len() && rows[row].right.is_some() {
                row += 1;
            }
            right_runs.push(begin..row);
        }
        Self {
            rows,
            right_runs,
            line_rows,
            max_no: max_no.max(1),
        }
    }
}

pub(super) fn split_blocks(plan: &SplitPlan) -> Vec<Range<usize>> {
    let mut blocks = Vec::new();
    let mut cursor = 0usize;
    for run in &plan.right_runs {
        if cursor < run.start {
            blocks.push(cursor..run.start);
        }
        blocks.push(run.clone());
        cursor = run.end;
    }
    if cursor < plan.rows.len() {
        blocks.push(cursor..plan.rows.len());
    }
    blocks
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct SplitSource {
    pub(super) ptr: usize,
    pub(super) len: usize,
    pub(super) lines: usize,
    pub(super) hash: u64,
}

pub(super) fn split_hash(lines: &[&DiffLine]) -> u64 {
    let mut hash = 0xcbf1_9ce4_8422_2325;
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

pub(super) struct SplitPlanCache {
    pub(super) source: SplitSource,
    pub(super) plan: Arc<SplitPlan>,
}

pub(super) fn cached_split_plan(
    ctx: &egui::Context,
    id: egui::Id,
    source: SplitSource,
    lines: &[&DiffLine],
) -> Arc<SplitPlan> {
    let cached = ctx.data_mut(|data| data.get_temp::<Arc<SplitPlanCache>>(id));
    if let Some(cache) = cached
        && cache.source == source
    {
        return Arc::clone(&cache.plan);
    }
    let plan = Arc::new(SplitPlan::build(lines));
    let cache = Arc::new(SplitPlanCache {
        source,
        plan: Arc::clone(&plan),
    });
    ctx.data_mut(|data| data.insert_temp(id, cache));
    plan
}

pub(super) fn left_row_height(
    row: SplitRowRef,
    lines: &[&DiffLine],
    metrics: &TextMetrics,
    wrap: WrapMetrics<'_>,
) -> f32 {
    row.left
        .and_then(|line| lines.get(line))
        .map_or(0.0, |line| wrap.height(code_text(line), metrics.row))
}

pub(super) struct SplitMetricsCache {
    pub(super) source: SplitSource,
    pub(super) wrap: Option<u32>,
    pub(super) font: FontId,
    pub(super) ppp: u32,
    pub(super) row: u32,
    pub(super) char_w: u32,
    pub(super) row_heights: Vec<f32>,
    pub(super) content_height: f32,
    pub(super) widest: f32,
}

pub(super) fn painted_line_height(
    ui: &egui::Ui,
    line: &DiffLine,
    metrics: &TextMetrics,
    wrap: Option<f32>,
) -> f32 {
    let mut job = line_job(line, ui.visuals(), &metrics.font, metrics.row);
    job.wrap.max_width = wrap.unwrap_or(f32::INFINITY);
    ui.painter().layout_job(job).rect.height().max(metrics.row)
}

#[expect(clippy::too_many_arguments)]
pub(super) fn static_split_metrics(
    ui: &egui::Ui,
    id: egui::Id,
    source: SplitSource,
    plan: &SplitPlan,
    lines: &[&DiffLine],
    metrics: &TextMetrics,
    wrap: bool,
    wrap_width: Option<f32>,
) -> Arc<SplitMetricsCache> {
    let ppp = ui.ctx().pixels_per_point().to_bits();
    let wrap_key = wrap_width.map(f32::to_bits);
    let cached = ui
        .ctx()
        .data_mut(|data| data.get_temp::<Arc<SplitMetricsCache>>(id));
    if let Some(cache) = cached
        && cache.source == source
        && cache.wrap == wrap_key
        && cache.font == metrics.font
        && cache.ppp == ppp
        && cache.row == metrics.row.to_bits()
        && cache.char_w == metrics.char_w.to_bits()
    {
        return cache;
    }
    let widths = char_widths(ui, &metrics.font, lines.iter().map(|line| code_text(line)));
    let mut row_heights = Vec::with_capacity(plan.rows.len());
    for row in &plan.rows {
        let left = row
            .left
            .and_then(|line| lines.get(line))
            .map_or(0.0, |line| {
                painted_line_height(ui, line, metrics, wrap_width)
            });
        let right = row
            .right
            .and_then(|line| lines.get(line))
            .map_or(0.0, |line| {
                painted_line_height(ui, line, metrics, wrap_width)
            });
        row_heights.push(left.max(right));
    }
    let content_height: f32 = row_heights.iter().sum();
    let widest = if wrap {
        0.0
    } else {
        lines
            .iter()
            .map(|line| line_width(code_text(line), metrics, &widths))
            .fold(0.0, f32::max)
    };
    let cache = Arc::new(SplitMetricsCache {
        source,
        wrap: wrap_key,
        font: metrics.font.clone(),
        ppp,
        row: metrics.row.to_bits(),
        char_w: metrics.char_w.to_bits(),
        row_heights,
        content_height,
        widest,
    });
    ui.ctx()
        .data_mut(|data| data.insert_temp(id, Arc::clone(&cache)));
    cache
}

pub(super) fn split_row_tops(
    plan: &SplitPlan,
    blocks: &[Range<usize>],
    block_heights: &[f32],
    row_heights: &[f32],
    tail_height: f32,
    tail_top: bool,
) -> Vec<f32> {
    let mut tops = vec![0.0f32; plan.rows.len()];
    let mut y = if tail_top { tail_height } else { 0.0 };
    if blocks.is_empty() {
        for (row, top) in tops.iter_mut().enumerate() {
            *top = y;
            y += row_heights.get(row).copied().unwrap_or(0.0);
        }
        return tops;
    }
    for (block, &target) in blocks.iter().zip(block_heights) {
        let mut used = 0.0;
        for row in block.clone() {
            tops[row] = y;
            let height = row_heights.get(row).copied().unwrap_or(0.0);
            y += height;
            used += height;
        }
        y += (target - used).max(0.0);
    }
    tops
}

pub(super) fn block_offset(
    blocks: &[Range<usize>],
    block_heights: &[f32],
    row_heights: &[f32],
    row: usize,
) -> f32 {
    let mut y = 0.0;
    for (block, &height) in blocks.iter().zip(block_heights) {
        if block.contains(&row) {
            y += row_heights[block.start..row].iter().sum::<f32>();
            return y;
        }
        y += height;
    }
    y
}
