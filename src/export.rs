//! export: the report as Markdown or as a self-contained HTML page.
//!
//! Both come from one small document model built from the [`Report`], not
//! from the terminal narrative, so they carry the same content and differ only
//! in markup. The Markdown is meant for a GitHub issue or PR (the call tree
//! and the diagnostics sit in collapsible sections); the HTML is one file
//! with its style inline, no scripts and no external requests.
//!
//! Everything that came from a contract, an RPC or a file goes through
//! `render::safe` before it is escaped for the markup, for the same reason
//! the terminal narrative does: it is text someone else chose.

use crate::RewindError;
use crate::render::{self, safe};
use crate::replay::PrngSeed;
use crate::report::Report;
use crate::story::Failure;

/// A run of inline text.
enum Span {
    Text(String),
    Code(String),
    Strong(String),
}

fn text(value: impl AsRef<str>) -> Span {
    Span::Text(safe(value.as_ref()))
}

fn code(value: impl AsRef<str>) -> Span {
    Span::Code(safe(value.as_ref()))
}

fn strong(value: impl AsRef<str>) -> Span {
    Span::Strong(safe(value.as_ref()))
}

/// One block of the document.
enum Block {
    Heading(u8, String),
    Paragraph(Vec<Span>),
    List(Vec<Vec<Span>>),
    Code(Vec<String>),
    Details {
        summary: String,
        open: bool,
        body: Vec<Block>,
    },
}

/// A rendered document: its title and its blocks.
struct Document {
    title: String,
    blocks: Vec<Block>,
}

/// The report as Markdown.
pub fn markdown(report: &Report) -> String {
    write_markdown(&report_document(report).blocks)
}

/// The report as one self-contained HTML page.
pub fn html(report: &Report) -> String {
    let document = report_document(report);
    write_html(&document.title, &document.blocks)
}

/// A failed run as Markdown.
pub fn markdown_error(error: &RewindError) -> String {
    write_markdown(&error_document(error).blocks)
}

/// A failed run as one self-contained HTML page.
pub fn html_error(error: &RewindError) -> String {
    let document = error_document(error);
    write_html(&document.title, &document.blocks)
}

fn error_document(error: &RewindError) -> Document {
    Document {
        title: "sororewind: the run failed".to_string(),
        blocks: vec![
            Block::Heading(1, "sororewind could not replay the transaction".to_string()),
            Block::Paragraph(vec![
                strong(error.stage().name()),
                text(" stage · "),
                code(error.kind()),
            ]),
            Block::Code(
                safe(&error.to_string())
                    .lines()
                    .map(str::to_string)
                    .collect(),
            ),
        ],
    }
}

fn report_document(report: &Report) -> Document {
    let mut blocks = vec![Block::Heading(
        1,
        format!(
            "sororewind {} on {}",
            report.hash.short(),
            safe(&report.network)
        ),
    )];
    let fee_bump = if report.fee_bump { " · fee-bump" } else { "" };
    blocks.push(Block::Paragraph(vec![text(format!(
        "ledger {} · protocol {} · {}{fee_bump}",
        report.ledger,
        report.protocol,
        render::utc(report.close_time)
    ))]));

    let verdict = if report.fidelity.matches {
        "✓ The replay matches the network."
    } else {
        "⚠ The replay diverged from the network."
    };
    blocks.push(Block::Paragraph(vec![strong(verdict)]));
    blocks.push(Block::List(vec![
        vec![text("on-chain: "), text(report.result.onchain.label())],
        vec![text("local: "), text(report.result.local.label())],
    ]));

    blocks.push(Block::Heading(2, "Findings".to_string()));
    blocks.push(Block::List(
        report
            .findings
            .iter()
            .map(|finding| {
                let severity = serde_json::to_value(finding.severity)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_default();
                let mut spans = vec![
                    strong(severity),
                    text(format!(" {} (evidence: ", finding.summary)),
                    code(finding.evidence.join(", ")),
                    text(")"),
                ];
                if let Some(guidance) = &finding.guidance {
                    spans.push(text(format!(" {}", guidance.explanation)));
                    for check in guidance.checks {
                        spans.push(text(format!(" Check (not verified): {check}")));
                    }
                }
                spans
            })
            .collect(),
    ));

    if !report.fidelity.matches && !report.probable_causes.is_empty() {
        blocks.push(Block::Heading(2, "Probable causes".to_string()));
        blocks.push(Block::List(
            report
                .probable_causes
                .iter()
                .map(|cause| vec![code(&cause.subject), text(format!(" — {}", cause.detail))])
                .collect(),
        ));
    }

    if let Some(failure) = &report.story.failure {
        blocks.push(Block::Heading(2, "Where it broke".to_string()));
        blocks.push(Block::List(failure_lines(failure)));
    }

    blocks.push(Block::Details {
        summary: "Call tree".to_string(),
        open: true,
        body: vec![Block::Code(render::plain_call_tree(&report.story.calls))],
    });
    blocks.push(Block::Details {
        summary: "Budget and footprint".to_string(),
        open: false,
        body: budget_and_footprint(report),
    });
    let diagnostics: Vec<Block> = [
        ("On-chain diagnostics", &report.diagnostics.onchain),
        ("Local diagnostics", &report.diagnostics.local),
    ]
    .into_iter()
    .filter(|(_, lines)| !lines.is_empty())
    .flat_map(|(title, lines)| {
        [
            Block::Paragraph(vec![strong(title)]),
            Block::Code(lines.iter().map(|line| safe(line)).collect()),
        ]
    })
    .collect();
    if !diagnostics.is_empty() {
        blocks.push(Block::Details {
            summary: "Diagnostic events".to_string(),
            open: false,
            body: diagnostics,
        });
    }

    if let Some(what_if) = &report.what_if {
        blocks.push(Block::Heading(
            2,
            format!("What-if protocol {} (non-faithful)", what_if.protocol),
        ));
        blocks.push(Block::List(
            render::what_if_lines(report, what_if)
                .into_iter()
                .map(|line| vec![text(line)])
                .collect(),
        ));
    }

    Document {
        title: format!("sororewind {}", report.hash.short()),
        blocks,
    }
}

fn failure_lines(failure: &Failure) -> Vec<Vec<Span>> {
    let mut lines = Vec::new();
    let mut headline = vec![strong(&failure.description)];
    if let Some(name) = &failure.name {
        headline.push(text(" · "));
        headline.push(code(name));
    }
    lines.push(headline);
    if !failure.message.is_empty() {
        lines.push(vec![text("message: "), code(&failure.message)]);
    }
    if let Some(contract) = &failure.contract {
        let place = match &failure.function {
            Some(function) => format!("{contract}::{function}"),
            None => contract.clone(),
        };
        let depth = failure
            .depth
            .map_or(String::new(), |depth| format!(" (depth {depth})"));
        lines.push(vec![text("in "), code(place), text(depth)]);
    }
    if let Some(cause) = &failure.cause {
        let mut line = vec![text("caused by "), strong(&cause.description)];
        if let Some(name) = &cause.name {
            line.push(text(" · "));
            line.push(code(name));
        }
        lines.push(line);
        if !cause.message.is_empty() {
            lines.push(vec![text("cause message: "), code(&cause.message)]);
        }
    }
    lines
}

fn budget_and_footprint(report: &Report) -> Vec<Block> {
    let b = &report.budget;
    let f = &report.footprint;
    let mut items = vec![
        vec![text(format!(
            "budget: {} / {} cpu instructions · {} / {} memory",
            render::grouped(b.cpu_insns),
            render::grouped(b.cpu_limit),
            render::human_bytes(b.mem_bytes),
            render::human_bytes(b.mem_limit)
        ))],
        vec![text(format!(
            "footprint: {} entries · {} drifted · {} ttl-drifted · {} missing · {} rebuilt",
            f.entries,
            f.drifted.len(),
            f.ttl_drifted.len(),
            f.missing,
            f.rebuilt.len()
        ))],
        vec![text(format!(
            "prng seed: {}",
            match report.prng_seed {
                PrngSeed::Derived => "derived",
                PrngSeed::Fixed => "fixed (apply order unknown)",
            }
        ))],
    ];
    let ttl: Vec<String> = f
        .ttl_drifted
        .iter()
        .map(|d| format!("{} ({})", d.entry, d.reason.describe()))
        .collect();
    for (label, entries) in [
        ("archived", &f.archived),
        ("drifted", &f.drifted),
        ("ttl drifted", &ttl),
        ("drift unknown", &f.drift_unknown),
        ("extension missing", &f.ext_missing),
        ("rebuilt from ledger metadata", &f.rebuilt),
    ] {
        for entry in entries {
            items.push(vec![text(format!("{label}: ")), code(entry)]);
        }
    }
    for status in &f.entry_status {
        items.push(vec![
            text("why "),
            code(&status.entry),
            text(format!(" ({}) — {}", status.reason.name(), status.detail)),
        ]);
    }
    vec![Block::List(items)]
}

// --- Markdown ---------------------------------------------------------------

fn write_markdown(blocks: &[Block]) -> String {
    let mut out = markdown_blocks(blocks).join("\n\n");
    out.push('\n');
    out
}

fn markdown_blocks(blocks: &[Block]) -> Vec<String> {
    blocks.iter().map(markdown_block).collect()
}

fn markdown_block(block: &Block) -> String {
    match block {
        Block::Heading(level, title) => {
            format!(
                "{} {}",
                "#".repeat(usize::from(*level)),
                escape_markdown(title)
            )
        }
        Block::Paragraph(spans) => markdown_spans(spans),
        Block::List(items) => items
            .iter()
            .map(|spans| format!("- {}", markdown_spans(spans)))
            .collect::<Vec<_>>()
            .join("\n"),
        Block::Code(lines) => {
            let joined = lines.join("\n");
            let fence = "`".repeat(longest_run(&joined, '`').max(2) + 1);
            format!("{fence}text\n{joined}\n{fence}")
        }
        Block::Details {
            summary,
            open,
            body,
        } => format!(
            "<details{}>\n<summary>{}</summary>\n\n{}\n\n</details>",
            if *open { " open" } else { "" },
            escape_html(summary),
            markdown_blocks(body).join("\n\n")
        ),
    }
}

fn markdown_spans(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| match span {
            Span::Text(value) => escape_markdown(value),
            Span::Strong(value) => format!("**{}**", escape_markdown(value)),
            Span::Code(value) => code_span(value),
        })
        .collect()
}

/// A code span that survives backticks inside it.
fn code_span(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let fence = "`".repeat(longest_run(value, '`') + 1);
    let pad = if value.starts_with('`') || value.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{value}{pad}{fence}")
}

fn longest_run(value: &str, target: char) -> usize {
    let (mut longest, mut run) = (0, 0);
    for c in value.chars() {
        run = if c == target { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// Text that cannot turn into markup: Markdown punctuation is escaped, and
/// `<` and `&` too, since GitHub passes raw HTML through.
fn escape_markdown(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '|' | '~' | '#' => {
                out.push('\\');
                out.push(c);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

// --- HTML -------------------------------------------------------------------

fn write_html(title: &str, blocks: &[Block]) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{}</title>\n<style>\n{STYLE}</style>\n</head>\n<body>\n<main>\n{}</main>\n\
         </body>\n</html>\n",
        escape_html(title),
        html_blocks(blocks)
    )
}

fn html_blocks(blocks: &[Block]) -> String {
    blocks.iter().map(html_block).collect()
}

fn html_block(block: &Block) -> String {
    match block {
        Block::Heading(level, title) => {
            format!("<h{level}>{}</h{level}>\n", escape_html(title))
        }
        Block::Paragraph(spans) => format!("<p>{}</p>\n", html_spans(spans)),
        Block::List(items) => {
            let items: String = items
                .iter()
                .map(|spans| format!("<li>{}</li>\n", html_spans(spans)))
                .collect();
            format!("<ul>\n{items}</ul>\n")
        }
        Block::Code(lines) => format!(
            "<pre><code>{}</code></pre>\n",
            escape_html(&lines.join("\n"))
        ),
        Block::Details {
            summary,
            open,
            body,
        } => format!(
            "<details{}>\n<summary>{}</summary>\n{}</details>\n",
            if *open { " open" } else { "" },
            escape_html(summary),
            html_blocks(body)
        ),
    }
}

fn html_spans(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|span| match span {
            Span::Text(value) => escape_html(value),
            Span::Strong(value) => format!("<strong>{}</strong>", escape_html(value)),
            Span::Code(value) => format!("<code>{}</code>", escape_html(value)),
        })
        .collect()
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

const STYLE: &str = "\
:root { color-scheme: light dark; --bg: #ffffff; --fg: #1f2328; --muted: #59636e; \
--rule: #d1d9e0; --code: #f6f8fa; }
@media (prefers-color-scheme: dark) { :root { --bg: #0d1117; --fg: #e6edf3; \
--muted: #9198a1; --rule: #3d444d; --code: #151b23; } }
body { background: var(--bg); color: var(--fg); margin: 0; \
font: 16px/1.5 system-ui, sans-serif; }
main { max-width: 56rem; margin: 0 auto; padding: 1.5rem 1rem; }
h1 { font-size: 1.5rem; } h2 { font-size: 1.15rem; border-bottom: 1px solid var(--rule); \
padding-bottom: .25rem; }
code, pre { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: .875rem; }
code { background: var(--code); padding: .1rem .3rem; border-radius: .25rem; }
pre { background: var(--code); padding: .75rem; border-radius: .375rem; overflow-x: auto; }
pre code { background: none; padding: 0; }
details { border: 1px solid var(--rule); border-radius: .375rem; padding: .25rem .75rem; \
margin: 1rem 0; }
summary { cursor: pointer; font-weight: 600; }
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_text_cannot_become_markup() {
        assert_eq!(escape_markdown("a_b <i>&*"), "a\\_b &lt;i&gt;&amp;\\*");
    }

    #[test]
    fn code_spans_and_fences_outgrow_the_backticks_inside() {
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`a"), "`` `a ``");
        let block = markdown_block(&Block::Code(vec!["```".to_string()]));
        assert!(block.starts_with("````text\n"), "{block}");
    }

    #[test]
    fn html_text_is_escaped() {
        assert_eq!(
            escape_html("<script>\"x\" & 'y'"),
            "&lt;script&gt;&quot;x&quot; &amp; &#39;y&#39;"
        );
    }
}
