use crate::engine::writer_to_string;
use crate::error::Result;
use crate::match_result::{MatchHit, ScanReport};
use csv::WriterBuilder;
use serde::Serialize;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Tsv,
    Json,
    Text,
}

impl ExportFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "csv" => Some(Self::Csv),
            "tsv" => Some(Self::Tsv),
            "json" => Some(Self::Json),
            "text" | "txt" => Some(Self::Text),
            _ => None,
        }
    }
}

pub fn export_hits(hits: &[MatchHit], format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Json => to_json(hits),
        ExportFormat::Csv => hits_to_delimited(hits, b','),
        ExportFormat::Tsv => hits_to_delimited(hits, b'\t'),
        ExportFormat::Text => Ok(hits
            .iter()
            .map(|h| {
                let col = h
                    .column_name
                    .clone()
                    .unwrap_or_else(|| format!("col_{}", h.column_index));
                match h.score {
                    Some(score) => format!(
                        "row={} column={} value={:?} pattern={:?} score={:.3}",
                        h.row, col, h.value, h.pattern, score
                    ),
                    None => format!(
                        "row={} column={} value={:?} pattern={:?}",
                        h.row, col, h.value, h.pattern
                    ),
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + if hits.is_empty() { "" } else { "\n" }),
    }
}

pub fn export_report(report: &ScanReport, format: ExportFormat) -> Result<String> {
    match format {
        ExportFormat::Json => to_json(report),
        ExportFormat::Text => Ok(format_report_text(report)),
        ExportFormat::Csv | ExportFormat::Tsv => {
            // Flat summary as key/value rows.
            let delim = if format == ExportFormat::Tsv {
                b'\t'
            } else {
                b','
            };
            let mut wtr = WriterBuilder::new()
                .delimiter(delim)
                .from_writer(Vec::new());
            wtr.write_record(["key", "value"])?;
            wtr.write_record(["total_rows", &report.total_rows.to_string()])?;
            wtr.write_record([
                "total_fields_scanned",
                &report.total_fields_scanned.to_string(),
            ])?;
            wtr.write_record(["total_matches", &report.total_matches.to_string()])?;
            wtr.write_record(["matched_rows", &report.matched_rows.to_string()])?;
            wtr.write_record(["columns_touched", &report.columns_touched.join("|")])?;
            for pc in &report.pattern_counts {
                wtr.write_record([format!("pattern:{}", pc.pattern), pc.count.to_string()])?;
            }
            writer_to_string(wtr)
        }
    }
}

fn hits_to_delimited(hits: &[MatchHit], delimiter: u8) -> Result<String> {
    let mut wtr = WriterBuilder::new()
        .delimiter(delimiter)
        .from_writer(Vec::new());
    wtr.write_record([
        "row",
        "column_index",
        "column_name",
        "value",
        "pattern",
        "score",
    ])?;
    for hit in hits {
        wtr.write_record([
            hit.row.to_string(),
            hit.column_index.to_string(),
            hit.column_name.clone().unwrap_or_default(),
            hit.value.clone(),
            hit.pattern.clone(),
            hit.score.map(|s| format!("{s:.6}")).unwrap_or_default(),
        ])?;
    }
    writer_to_string(wtr)
}

fn to_json<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|e| {
        crate::error::CsvCoincidenceError::Message(format!("JSON serialization failed: {e}"))
    })
}

fn format_report_text(report: &ScanReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("total_rows: {}\n", report.total_rows));
    out.push_str(&format!(
        "total_fields_scanned: {}\n",
        report.total_fields_scanned
    ));
    out.push_str(&format!("total_matches: {}\n", report.total_matches));
    out.push_str(&format!("matched_rows: {}\n", report.matched_rows));
    out.push_str(&format!(
        "columns_touched: {}\n",
        report.columns_touched.join(", ")
    ));
    out.push_str("pattern_counts:\n");
    for pc in &report.pattern_counts {
        out.push_str(&format!("  - {}: {}\n", pc.pattern, pc.count));
    }
    out.push_str("top_values:\n");
    for vc in &report.top_values {
        out.push_str(&format!("  - {:?}: {}\n", vc.value, vc.count));
    }
    out
}
