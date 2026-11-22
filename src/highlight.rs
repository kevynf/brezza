use std::path::Path;
use std::sync::{LazyLock, OnceLock};

use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::model::{Syntax, SyntaxSpans};

const CAPTURES: [(&str, Syntax); 18] = [
    ("comment", Syntax::Comment),
    ("keyword", Syntax::Keyword),
    ("tag", Syntax::Keyword),
    ("variable.builtin", Syntax::Keyword),
    ("string", Syntax::String),
    ("escape", Syntax::String),
    ("character", Syntax::String),
    ("number", Syntax::Literal),
    ("constant", Syntax::Literal),
    ("boolean", Syntax::Literal),
    ("type", Syntax::Literal),
    ("constructor", Syntax::Literal),
    ("attribute", Syntax::Literal),
    ("label", Syntax::Literal),
    ("function", Syntax::Function),
    ("property", Syntax::Function),
    ("punctuation", Syntax::Punctuation),
    ("operator", Syntax::Punctuation),
];

struct Grammar {
    extensions: &'static [&'static str],
    language: Language,
    name: &'static str,
    highlights: String,
    injections: String,
    locals: String,
    config: OnceLock<Option<HighlightConfiguration>>,
}

impl Grammar {
    fn config(&self) -> Option<&HighlightConfiguration> {
        self.config
            .get_or_init(|| {
                let mut config = HighlightConfiguration::new(
                    self.language.clone(),
                    self.name,
                    &self.highlights,
                    &self.injections,
                    &self.locals,
                )
                .ok()?;
                let names: Vec<&str> = CAPTURES.iter().map(|(name, _)| *name).collect();
                config.configure(&names);
                Some(config)
            })
            .as_ref()
    }
}

type Spec<'a> = (
    &'static [&'static str],
    Language,
    &'static str,
    String,
    &'a str,
    &'a str,
);

static GRAMMARS: LazyLock<Vec<Grammar>> = LazyLock::new(|| {
    let js = tree_sitter_javascript::HIGHLIGHT_QUERY;
    let jsx = tree_sitter_javascript::JSX_HIGHLIGHT_QUERY;
    let ts = tree_sitter_typescript::HIGHLIGHTS_QUERY;
    let ts_locals = [
        tree_sitter_typescript::LOCALS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    ]
    .concat();
    let specs: Vec<Spec<'_>> = vec![
        (
            &["rs"],
            tree_sitter_rust::LANGUAGE.into(),
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY.into(),
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ),
        (
            &["js", "mjs", "cjs", "jsx"],
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript",
            [jsx, js].concat(),
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        (
            &["ts", "mts", "cts"],
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
            [ts, js].concat(),
            "",
            &ts_locals,
        ),
        (
            &["tsx"],
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            "tsx",
            [ts, jsx, js].concat(),
            "",
            &ts_locals,
        ),
        (
            &["py", "pyi"],
            tree_sitter_python::LANGUAGE.into(),
            "python",
            tree_sitter_python::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["json", "jsonc"],
            tree_sitter_json::LANGUAGE.into(),
            "json",
            tree_sitter_json::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["toml"],
            tree_sitter_toml_ng::LANGUAGE.into(),
            "toml",
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["css"],
            tree_sitter_css::LANGUAGE.into(),
            "css",
            tree_sitter_css::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["html", "htm"],
            tree_sitter_html::LANGUAGE.into(),
            "html",
            tree_sitter_html::HIGHLIGHTS_QUERY.into(),
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        ),
        (
            &["sh", "bash", "zsh"],
            tree_sitter_bash::LANGUAGE.into(),
            "bash",
            tree_sitter_bash::HIGHLIGHT_QUERY.into(),
            "",
            "",
        ),
        (
            &["c", "h"],
            tree_sitter_c::LANGUAGE.into(),
            "c",
            tree_sitter_c::HIGHLIGHT_QUERY.into(),
            "",
            "",
        ),
        (
            &["cpp", "cc", "cxx", "hpp", "hh", "hxx"],
            tree_sitter_cpp::LANGUAGE.into(),
            "cpp",
            [
                tree_sitter_cpp::HIGHLIGHT_QUERY,
                tree_sitter_c::HIGHLIGHT_QUERY,
            ]
            .concat(),
            "",
            "",
        ),
        (
            &["go"],
            tree_sitter_go::LANGUAGE.into(),
            "go",
            tree_sitter_go::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["java"],
            tree_sitter_java::LANGUAGE.into(),
            "java",
            tree_sitter_java::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["yml", "yaml"],
            tree_sitter_yaml::LANGUAGE.into(),
            "yaml",
            tree_sitter_yaml::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
        (
            &["cs"],
            tree_sitter_c_sharp::LANGUAGE.into(),
            "c_sharp",
            tree_sitter_c_sharp::HIGHLIGHTS_QUERY.into(),
            "",
            "",
        ),
    ];
    specs
        .into_iter()
        .map(
            |(extensions, language, name, highlights, injections, locals)| Grammar {
                extensions,
                language,
                name,
                highlights,
                injections: injections.to_owned(),
                locals: locals.to_owned(),
                config: OnceLock::new(),
            },
        )
        .collect()
});

pub fn highlight(path: &str, text: &str) -> Option<Vec<SyntaxSpans>> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    let config = GRAMMARS
        .iter()
        .find(|g| g.extensions.contains(&extension.as_str()))?
        .config()?;
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let mut lines = vec![SyntaxSpans::new(); starts.len()];
    let mut stack = Vec::new();
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, text.as_bytes(), None, None, |name| {
            GRAMMARS
                .iter()
                .find(|g| g.name == name)
                .and_then(Grammar::config)
        })
        .ok()?;
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(h) => stack.push(CAPTURES[h.0].1),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let Some(&kind) = stack.last() else {
                    continue;
                };
                let mut line = starts.partition_point(|&s| s <= start) - 1;
                let mut from = start;
                while from < end && line < starts.len() {
                    let line_end = starts.get(line + 1).map_or(text.len(), |s| s - 1);
                    let to = end.min(line_end);
                    if to > from {
                        lines[line].push((from - starts[line]..to - starts[line], kind));
                    }
                    line += 1;
                    from = starts.get(line).copied().unwrap_or(end);
                }
            }
        }
    }
    Some(lines)
}
