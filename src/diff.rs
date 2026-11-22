use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use imara_diff::{Algorithm, Diff, InternedInput};

use crate::highlight::highlight;
use crate::model::{DiffBody, DiffLine, Hunk, LineKind, Oid, SyntaxSpans};

pub const CONTEXT: u32 = 3;
pub const FULL: u32 = u32::MAX;
const MAX_BYTES: usize = 2 << 20;
const MAX_SUMMARIES: usize = 4096;
const MAX_BODIES: usize = 512;
const MAX_SPANS: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stats {
    Text { added: u32, removed: u32 },
    Binary,
    TooLarge,
}

impl Stats {
    pub fn stats(self) -> Option<(u32, u32)> {
        match self {
            Stats::Text { added, removed } => Some((added, removed)),
            Stats::Binary | Stats::TooLarge => None,
        }
    }
}

pub struct Cache {
    summaries: HashMap<u64, Stats>,
    bodies: HashMap<(u64, u32), DiffBody>,
    spans: Spans,
}

impl Cache {
    pub fn new(hash: gix::hash::Kind) -> Self {
        Self {
            summaries: HashMap::new(),
            bodies: HashMap::new(),
            spans: Spans::new(hash),
        }
    }

    pub fn stats(&self, version: u64) -> Option<Stats> {
        self.summaries.get(&version).copied()
    }

    pub fn remember(&mut self, version: u64, stats: Stats) {
        if self.summaries.len() >= MAX_SUMMARIES {
            self.summaries.clear();
        }
        self.summaries.insert(version, stats);
    }

    pub fn body(&self, version: u64, context: u32) -> Option<DiffBody> {
        self.bodies.get(&(version, context)).cloned()
    }

    pub fn preview(&mut self, path: &str, old: &[u8], new: &[u8], context: u32) -> DiffBody {
        diff_blobs(path, old, new, context, Some(&mut self.spans))
    }

    pub fn diff(
        &mut self,
        version: u64,
        path: &str,
        old: &[u8],
        new: &[u8],
        context: u32,
    ) -> DiffBody {
        let key = (version, context);
        if let Some(body) = self.bodies.get(&key) {
            return body.clone();
        }
        let body = diff_blobs(path, old, new, context, Some(&mut self.spans));
        if self.bodies.len() >= MAX_BODIES {
            self.bodies.clear();
        }
        self.bodies.insert(key, body.clone());
        body
    }
}

pub fn measure(path: &str, old: &[u8], new: &[u8]) -> Stats {
    summarize(&diff_blobs(path, old, new, CONTEXT, None))
}

pub fn summarize(body: &DiffBody) -> Stats {
    let hunks = match body {
        DiffBody::Text(hunks) => hunks,
        DiffBody::Binary => return Stats::Binary,
        DiffBody::TooLarge => return Stats::TooLarge,
        DiffBody::Pending => {
            return Stats::Text {
                added: 0,
                removed: 0,
            };
        }
    };
    let (mut added, mut removed) = (0, 0);
    for line in hunks.iter().flat_map(|h| &h.lines) {
        match line.kind {
            LineKind::Added => added += 1,
            LineKind::Removed => removed += 1,
            LineKind::Context => {}
        }
    }
    Stats::Text { added, removed }
}

fn diff_blobs(
    path: &str,
    old: &[u8],
    new: &[u8],
    context: u32,
    mut spans: Option<&mut Spans>,
) -> DiffBody {
    if old.len() > MAX_BYTES || new.len() > MAX_BYTES {
        return DiffBody::TooLarge;
    }
    if old.contains(&0) || new.contains(&0) {
        return DiffBody::Binary;
    }
    let old_text = String::from_utf8_lossy(old);
    let new_text = String::from_utf8_lossy(new);
    let (old_lines, old_eol) = split_lines(&old_text);
    let (new_lines, new_eol) = split_lines(&new_text);

    let mut input = InternedInput::default();
    input.update_before(old_lines.iter().copied());
    input.update_after(new_lines.iter().copied());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut groups: Vec<(u32, u32, u32, u32)> = Vec::new();
    for h in diff.hunks() {
        let start_old = h.before.start.saturating_sub(context);
        let end_old = h
            .before
            .end
            .saturating_add(context)
            .min(old_lines.len() as u32);
        let start_new = h.after.start.saturating_sub(h.before.start - start_old);
        let end_new = (h.after.end + (end_old - h.before.end)).min(new_lines.len() as u32);
        match groups.last_mut() {
            Some(last) if start_old <= last.1 => {
                last.1 = end_old;
                last.3 = end_new;
            }
            _ => groups.push((start_old, end_old, start_new, end_new)),
        }
    }
    if groups.is_empty() && context == FULL && !old_lines.is_empty() {
        groups.push((0, old_lines.len() as u32, 0, new_lines.len() as u32));
    }

    for (os, oe, ns, ne) in groups {
        let mut lines = Vec::new();
        let (mut o, mut n) = (os, ns);
        while o < oe || n < ne {
            if o < oe && diff.is_removed(o) {
                lines.push(line(
                    LineKind::Removed,
                    Some(o),
                    None,
                    old_lines[o as usize],
                ));
                o += 1;
            } else if n < ne && diff.is_added(n) {
                lines.push(line(LineKind::Added, None, Some(n), new_lines[n as usize]));
                n += 1;
            } else {
                lines.push(line(
                    LineKind::Context,
                    Some(o),
                    Some(n),
                    old_lines[o as usize],
                ));
                o += 1;
                n += 1;
            }
        }
        mark_eol(
            &mut lines,
            old_lines.len() as u32,
            old_eol,
            new_lines.len() as u32,
            new_eol,
        );
        emphasize(&mut lines);
        hunks.push(Hunk {
            old_start: if oe > os { os + 1 } else { os },
            old_len: oe - os,
            new_start: if ne > ns { ns + 1 } else { ns },
            new_len: ne - ns,
            lines,
        });
    }

    let old_syntax = spans
        .as_mut()
        .and_then(|spans| spans.get(path, old, &old_text));
    let new_syntax = spans
        .as_mut()
        .and_then(|spans| spans.get(path, new, &new_text));
    for line in hunks.iter_mut().flat_map(|h| &mut h.lines) {
        let (syntax, no) = match line.kind {
            LineKind::Added => (&new_syntax, line.new_no),
            _ => (&old_syntax, line.old_no),
        };
        if let (Some(spans), Some(no)) = (syntax, no) {
            line.syntax = spans.get(no as usize - 1).cloned().unwrap_or_default();
        }
    }
    DiffBody::Text(Arc::new(hunks))
}

struct Spans {
    hash: gix::hash::Kind,
    cache: HashMap<(String, Oid), Arc<Vec<SyntaxSpans>>>,
}

impl Spans {
    fn new(hash: gix::hash::Kind) -> Self {
        Self {
            hash,
            cache: HashMap::new(),
        }
    }

    fn get(&mut self, path: &str, bytes: &[u8], text: &str) -> Option<Arc<Vec<SyntaxSpans>>> {
        let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        let id = gix::objs::compute_hash(self.hash, gix::objs::Kind::Blob, bytes).ok()?;
        if let Some(spans) = self.cache.get(&(extension.clone(), id)) {
            return Some(spans.clone());
        }
        let spans = Arc::new(highlight(path, text)?);
        if self.cache.len() >= MAX_SPANS {
            self.cache.clear();
        }
        self.cache.insert((extension, id), spans.clone());
        Some(spans)
    }
}

fn split_lines(text: &str) -> (Vec<&str>, bool) {
    if text.is_empty() {
        return (Vec::new(), true);
    }
    let eol = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    (body.split('\n').collect(), eol)
}

fn line(kind: LineKind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
    DiffLine {
        kind,
        old_no: old.map(|n| n + 1),
        new_no: new.map(|n| n + 1),
        text: text.to_owned(),
        emphasis: Vec::new(),
        syntax: Vec::new(),
        no_eol: false,
    }
}

fn mark_eol(lines: &mut [DiffLine], old_len: u32, old_eol: bool, new_len: u32, new_eol: bool) {
    for l in lines {
        let old_last = !old_eol && l.old_no == Some(old_len);
        let new_last = !new_eol && l.new_no == Some(new_len);
        l.no_eol = match l.kind {
            LineKind::Removed => old_last,
            LineKind::Added => new_last,
            LineKind::Context => old_last || new_last,
        };
    }
}

fn emphasize(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        let removed_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Removed {
            i += 1;
        }
        let added_start = i;
        while i < lines.len() && lines[i].kind == LineKind::Added {
            i += 1;
        }
        let pairs = (added_start - removed_start).min(i - added_start);
        for k in 0..pairs {
            let (old, new) = lines.split_at_mut(added_start);
            word_diff(&mut old[removed_start + k], &mut new[k]);
        }
        if i == removed_start {
            i += 1;
        }
    }
}

fn word_diff(old: &mut DiffLine, new: &mut DiffLine) {
    let a = tokens(&old.text);
    let b = tokens(&new.text);
    let mut input = InternedInput::default();
    input.update_before(a.iter().map(|(_, t)| *t));
    input.update_after(b.iter().map(|(_, t)| *t));
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let ranges = |toks: &[(usize, &str)], changed: &dyn Fn(u32) -> bool| {
        let mut out: Vec<std::ops::Range<usize>> = Vec::new();
        for (i, (start, t)) in toks.iter().enumerate() {
            if !changed(i as u32) {
                continue;
            }
            let end = start + t.len();
            match out.last_mut() {
                Some(r) if r.end == *start => r.end = end,
                _ => out.push(*start..end),
            }
        }
        out
    };
    let old_ranges = ranges(&a, &|i| diff.is_removed(i));
    let new_ranges = ranges(&b, &|i| diff.is_added(i));
    let changed: usize = old_ranges.iter().chain(&new_ranges).map(|r| r.len()).sum();
    if changed * 2 > old.text.len() + new.text.len() {
        return;
    }
    old.emphasis = old_ranges;
    new.emphasis = new_ranges;
}

fn tokens(text: &str) -> Vec<(usize, &str)> {
    let class = |c: char| {
        if c.is_alphanumeric() && c.is_ascii() || c == '_' {
            1
        } else if c.is_whitespace() {
            2
        } else {
            0
        }
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev = None;
    for (i, c) in text.char_indices() {
        let k = class(c);
        if i > start && (k == 0 || prev != Some(k)) {
            out.push((start, &text[start..i]));
            start = i;
        }
        prev = Some(k);
    }
    if start < text.len() {
        out.push((start, &text[start..]));
    }
    out
}

pub fn hunk_patch(path: &str, hunk: &Hunk) -> String {
    let mut out = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{},{} +{},{} @@\n",
        hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len
    );
    for l in &hunk.lines {
        out.push(match l.kind {
            LineKind::Context => ' ',
            LineKind::Added => '+',
            LineKind::Removed => '-',
        });
        out.push_str(&l.text);
        out.push('\n');
        if l.no_eol {
            out.push_str("\\ No newline at end of file\n");
        }
    }
    out
}
