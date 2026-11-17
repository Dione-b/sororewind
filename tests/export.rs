//! The Markdown and HTML exports, over recorded fixtures.
//!
//! Two fixtures are pinned byte for byte in both formats; the rest of the
//! file checks what a snapshot would not catch: errors in the chosen format,
//! and that text chosen by a contract cannot become markup.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use sororewind::RewindError;
use sororewind::export;
use sororewind::render::{self, Format};

#[tokio::test]
async fn contract_error_in_markdown_and_html() {
    let report = common::replay("contract-error").await.unwrap();
    insta::assert_snapshot!("contract-error-markdown", export::markdown(&report));
    insta::assert_snapshot!("contract-error-html", export::html(&report));
}

#[tokio::test]
async fn nested_calls_in_markdown_and_html() {
    let report = common::replay("nested-contract-error").await.unwrap();
    insta::assert_snapshot!("nested-contract-error-markdown", export::markdown(&report));
    insta::assert_snapshot!("nested-contract-error-html", export::html(&report));
}

#[tokio::test]
async fn a_divergence_lists_its_probable_causes() {
    let report = common::replay("contract-error").await.unwrap();
    let mut diverged = report.clone();
    diverged.fidelity =
        sororewind::replay::Fidelity::diverged(sororewind::replay::Divergence::Result);
    diverged.probable_causes = vec![sororewind::culprit::Culprit {
        kind: sororewind::culprit::CulpritKind::EntryDrifted,
        subject: "account A".to_string(),
        detail: "changed after the transaction ran".to_string(),
    }];
    let markdown = export::markdown(&diverged);
    assert!(markdown.contains("The replay diverged"), "{markdown}");
    assert!(markdown.contains("## Probable causes"), "{markdown}");
    assert!(!export::markdown(&report).contains("Probable causes"));
}

#[tokio::test]
async fn text_from_a_contract_cannot_become_markup() {
    let mut report = common::replay("contract-error").await.unwrap();
    let hostile = "</code><script>alert(1)</script> `x` [link](http://evil) \u{1b}[31m";
    report.story.failure.as_mut().unwrap().message = hostile.to_string();

    let html = export::html(&report);
    assert!(!html.contains("<script>"), "{html}");
    assert!(!html.contains('\u{1b}'));
    assert!(html.contains("&lt;script&gt;"), "{html}");

    let markdown = export::markdown(&report);
    // Inside a code span the tag is literal text; anywhere else it would be
    // markup.
    for line in markdown.lines().filter(|line| line.contains("<script>")) {
        assert!(line.starts_with("- message: ``"), "{line}");
    }
    assert!(!markdown.contains('\u{1b}'));
}

#[test]
fn errors_are_rendered_in_the_chosen_format() {
    let error = RewindError::LedgerUnavailable { sequence: 7 };

    // Text goes to stderr only: nothing for stdout.
    assert_eq!(render::error(&error, Format::Text), None);

    let json = render::error(&error, Format::Json).unwrap();
    assert!(json.contains("\"stage\": \"reconstruct\""), "{json}");

    let markdown = render::error(&error, Format::Markdown).unwrap();
    insta::assert_snapshot!("error-markdown", markdown);
    let html = render::error(&error, Format::Html).unwrap();
    insta::assert_snapshot!("error-html", html);
}
