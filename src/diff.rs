//! Comparison of saved replay reports.

use std::fmt::Write;
use std::path::Path;

use serde_json::Value;
use sororewind::{Result, RewindError};

pub fn compare(before: &Path, after: &Path) -> Result<(String, bool)> {
    let read = |path: &Path| -> Result<Value> {
        let error = |reason: String| invalid(format!("{}: {reason}", path.display()));
        let bytes = std::fs::read(path).map_err(|err| error(err.to_string()))?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|err| {
            error(format!(
                "invalid JSON at line {}, column {}",
                err.line(),
                err.column()
            ))
        })?;
        if version(&value).is_none() {
            return Err(error(
                "expected a report with a positive integer schemaVersion".into(),
            ));
        }
        if !value["fidelity"]["match"].is_boolean() || !value["calls"].is_array() {
            return Err(error(
                "expected a replay report with fidelity and calls".into(),
            ));
        }
        Ok(value)
    };

    let before_report = read(before)?;
    let after_report = read(after)?;
    if version(&before_report) != version(&after_report) {
        return Err(invalid(format!(
            "schemaVersion differs ({} vs {})",
            before_report["schemaVersion"], after_report["schemaVersion"]
        )));
    }

    let sections = [
        (
            "Transaction",
            &[
                "hash",
                "network",
                "ledger",
                "protocol",
                "closeTime",
                "feeBump",
            ][..],
        ),
        (
            "Outcome",
            &[
                "status",
                "fidelity",
                "result",
                "fee",
                "prngSeed",
                "trust",
                "probableCauses",
            ],
        ),
        ("Failure", &["failure"]),
        ("Call tree", &["calls"]),
        ("Events", &["diagnostics"]),
        ("Budget", &["budget"]),
        ("Footprint", &["footprint"]),
    ];
    let mut output = String::new();
    for (section, fields) in sections {
        let mut changes = String::new();
        for field in fields {
            let a = before_report.get(field);
            let b = after_report.get(field);
            if a != b {
                let before = a.map_or_else(|| "(missing)".into(), Value::to_string);
                let after = b.map_or_else(|| "(missing)".into(), Value::to_string);
                let _ = writeln!(
                    changes,
                    "  {field}\n    before: {before}\n    after:  {after}"
                );
            }
        }
        if !changes.is_empty() {
            let _ = writeln!(output, "{section}\n{changes}");
        }
    }
    if output.is_empty() {
        Ok(("Reports are equivalent.\n".into(), false))
    } else {
        Ok((output, true))
    }
}

fn version(report: &Value) -> Option<u64> {
    report["schemaVersion"]
        .as_u64()
        .filter(|version| *version > 0)
}

fn invalid(reason: String) -> RewindError {
    RewindError::InvalidOption {
        name: "diff",
        reason,
    }
}
