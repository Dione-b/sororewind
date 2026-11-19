//! Keeps the README options table in step with the public CLI.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;

#[test]
fn readme_options_match_public_cli_flags() {
    let mut command = sororewind::cli::Cli::command();
    command.build();
    let cli: BTreeSet<_> = command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set())
        .filter_map(|arg| arg.get_long())
        .collect();

    let readme = include_str!("../README.md");
    let options = readme.split_once("\n## Options\n").unwrap().1;
    let options = options.split("\n## ").next().unwrap();
    let documented: BTreeSet<_> = options
        .lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|line| line.split_once('`').map(|(option, _)| option))
        .flat_map(str::split_whitespace)
        .filter_map(|word| word.strip_prefix("--"))
        .collect();

    let missing: Vec<_> = cli.difference(&documented).collect();
    let stale: Vec<_> = documented.difference(&cli).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "update README Options to match --help; missing: {missing:?}, stale: {stale:?}"
    );
}
