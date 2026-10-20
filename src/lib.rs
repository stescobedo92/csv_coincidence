//! # csv_coincidence
//!
//! Search, count, filter, replace, and report pattern matches inside CSV files.
//!
//! The crate exposes both a library API and an optional CLI binary
//! (`csv-coincidence`, feature `cli`).

mod engine;
mod error;
mod export;
mod match_result;
mod options;
mod patterns;

pub use engine::{
    build_reader, build_report, count_matches_reader, dedupe_rows_reader, dedupe_rows_to_writer,
    filter_rows_reader, filter_rows_to_writer, find_matches_iter_reader, find_matches_reader,
    open_reader_from_path, replace_matches_reader, replace_matches_to_writer, scan_stats_reader,
    write_output, MatchHitIter,
};
pub use error::{CsvCoincidenceError, Result};
pub use export::{export_hits, export_report, ExportFormat};
pub use match_result::{MatchHit, PatternCount, ScanReport, ValueCount};
pub use options::{
    ColumnRule, ColumnSelector, CsvOptions, LogicOp, MatchMode, ReplaceOptions, SearchOptions,
};
pub use patterns::{
    builtin_pattern, builtin_patterns, is_valid_credit_card, is_valid_dni, is_valid_iban,
    is_valid_nie, parse_patterns, read_pattern_file, CREDIT_CARD_PATTERN, DATE_ISO_PATTERN,
    DNI_PATTERN, EMAIL_PATTERN, IBAN_PATTERN, IPV4_PATTERN, NIE_PATTERN, PHONE_ES_PATTERN,
    URL_PATTERN, UUID_PATTERN,
};

use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Find partial matches in a CSV file path using a single regex pattern.
///
/// Convenience wrapper around [`find_matches`] that returns only field values.
pub fn find_partial_matches(file_path: &str, regex_pattern: &str) -> Result<Vec<String>> {
    let hits = find_matches(file_path, SearchOptions::new([regex_pattern]))?;
    Ok(hits.into_iter().map(|h| h.value).collect())
}

/// Count occurrences of a pattern in a CSV file path.
pub fn count_coincidences(file_path: &str, pattern: &str) -> Result<usize> {
    count_matches(file_path, SearchOptions::new([pattern]))
}

/// Replace matching fields with `[MERGED]` and keep only matched rows.
///
/// Prefer [`replace_matches`] for custom replacement text and options.
pub fn merge_coincidence(file_path: &str, pattern: &str) -> Result<String> {
    replace_matches(
        file_path,
        ReplaceOptions::new([pattern], "[MERGED]").keep_headers(false),
    )
}

/// Find matches from a file path.
pub fn find_matches(file_path: impl AsRef<Path>, options: SearchOptions) -> Result<Vec<MatchHit>> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    find_matches_reader(reader, &options)
}

/// Find matches from any `Read` source (file, stdin buffer, memory).
pub fn find_matches_from_read<R: Read>(reader: R, options: SearchOptions) -> Result<Vec<MatchHit>> {
    let csv_reader = build_reader(reader, &options);
    find_matches_reader(csv_reader, &options)
}

/// Stream matches from a file path without materialising all hits.
pub fn find_matches_iter(
    file_path: impl AsRef<Path>,
    options: SearchOptions,
) -> Result<MatchHitIter<File>> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    find_matches_iter_reader(reader, &options)
}

/// Stream matches from any `Read` source without materialising all hits.
pub fn find_matches_iter_from_read<R: Read>(
    reader: R,
    options: SearchOptions,
) -> Result<MatchHitIter<R>> {
    let csv_reader = build_reader(reader, &options);
    find_matches_iter_reader(csv_reader, &options)
}

/// Count matches from a file path.
pub fn count_matches(file_path: impl AsRef<Path>, options: SearchOptions) -> Result<usize> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    count_matches_reader(reader, &options)
}

/// Filter rows from a file path.
pub fn filter_rows(
    file_path: impl AsRef<Path>,
    options: SearchOptions,
    keep_matches: bool,
) -> Result<String> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    filter_rows_reader(reader, &options, keep_matches)
}

/// Replace matches from a file path.
pub fn replace_matches(file_path: impl AsRef<Path>, options: ReplaceOptions) -> Result<String> {
    let reader = open_reader_from_path(file_path.as_ref(), &options.search)?;
    replace_matches_reader(reader, &options)
}

/// Produce a scan report from a file path.
pub fn report_matches(file_path: impl AsRef<Path>, options: SearchOptions) -> Result<ScanReport> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    let (hits, rows, fields) = scan_stats_reader(reader, &options)?;
    Ok(build_report(&hits, rows, fields))
}

/// Deduplicate rows from a file path using matched-column fingerprints.
pub fn dedupe_rows(file_path: impl AsRef<Path>, options: SearchOptions) -> Result<String> {
    let reader = open_reader_from_path(file_path.as_ref(), &options)?;
    dedupe_rows_reader(reader, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn sample_csv() -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(
            file,
            "name,city,email\nJhon,Madrid,jhon@example.com\nMarta,Berlin,marta@test.org\nanna,Paris,anna@example.com\nJhon,Madrid,jhon@example.com"
        )
        .unwrap();
        file
    }

    fn sample_path() -> (NamedTempFile, std::path::PathBuf) {
        let file = sample_csv();
        let path = file.path().with_extension("csv");
        std::fs::copy(file.path(), &path).unwrap();
        (file, path)
    }

    #[test]
    fn finds_partial_matches() {
        let (_file, path) = sample_path();
        let matches = find_partial_matches(path.to_str().unwrap(), r"^[A-Z][a-z]*").unwrap();
        assert!(matches.contains(&"Jhon".to_string()));
        assert!(matches.contains(&"Marta".to_string()));
        assert!(matches.contains(&"Madrid".to_string()));
        assert!(matches.contains(&"Berlin".to_string()));
        assert!(matches.contains(&"Paris".to_string()));
    }

    #[test]
    fn rejects_invalid_extension() {
        let err = find_partial_matches("test_data.txt", r"^[A-Z][a-z]*").unwrap_err();
        assert!(matches!(err, CsvCoincidenceError::InvalidExtension(_)));
    }

    #[test]
    fn counts_and_reports_with_column_filter() {
        let (_file, path) = sample_path();
        let options = SearchOptions::new([r"example\.com"])
            .columns(ColumnSelector::Names(vec!["email".into()]));
        let count = count_matches(&path, options.clone()).unwrap();
        assert_eq!(count, 3);

        let report = report_matches(&path, options).unwrap();
        assert_eq!(report.total_matches, 3);
        assert!(report.columns_touched.contains(&"email".to_string()));
    }

    #[test]
    fn replaces_with_custom_text() {
        let (_file, path) = sample_path();
        let out = replace_matches(
            &path,
            ReplaceOptions::new([r"@.*"], "[REDACTED]")
                .search(
                    SearchOptions::new([r"@.*"])
                        .columns(ColumnSelector::Names(vec!["email".into()])),
                )
                .only_matched_rows(false),
        )
        .unwrap();
        assert!(out.contains("[REDACTED]"));
        assert!(out.contains("name,city,email"));
    }

    #[test]
    fn filters_and_dedupes() {
        let (_file, path) = sample_path();
        let options = SearchOptions::new(["Jhon"]);
        let filtered = filter_rows(&path, options.clone(), true).unwrap();
        assert_eq!(filtered.lines().count(), 3); // header + 2 Jhon rows

        let deduped = dedupe_rows(&path, options).unwrap();
        assert_eq!(deduped.lines().count(), 4); // header + 3 unique rows
    }

    #[test]
    fn fuzzy_match_works() {
        let (_file, path) = sample_path();
        let options = SearchOptions::new(["John"])
            .mode(MatchMode::Fuzzy { threshold: 0.5 })
            .columns(ColumnSelector::Names(vec!["name".into()]));
        let hits = find_matches(&path, options).unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.value == "Jhon"));
    }

    #[test]
    fn reads_from_memory() {
        let data = b"id,name\n1,Alpha\n2,Beta\n";
        let options = SearchOptions::new([r"^A"]);
        let hits = find_matches_from_read(&data[..], options).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].value, "Alpha");
    }

    #[test]
    fn merge_coincidence_compat() {
        let (_file, path) = sample_path();
        let merged = merge_coincidence(path.to_str().unwrap(), r"^Jhon$").unwrap();
        assert!(merged.contains("[MERGED]"));
        assert!(!merged.contains("name,city,email"));
    }

    #[test]
    fn exports_json_and_csv() {
        let hits = vec![MatchHit {
            row: 1,
            column_index: 0,
            column_name: Some("name".into()),
            value: "Jhon".into(),
            pattern: "Jhon".into(),
            score: None,
            matched_text: "Jhon".into(),
            start: 0,
            end: 4,
            captures: Vec::new(),
        }];
        let json = export_hits(&hits, ExportFormat::Json).unwrap();
        assert!(json.contains("Jhon"));
        let csv = export_hits(&hits, ExportFormat::Csv).unwrap();
        assert!(csv.contains("row,column_index"));
        assert!(csv.contains("matched_text"));
    }
}
