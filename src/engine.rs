use crate::error::{CsvCoincidenceError, Result};
use crate::match_result::{MatchHit, PatternCount, ScanReport, ValueCount};
use crate::options::{ColumnSelector, MatchMode, ReplaceOptions, SearchOptions};
use csv::{Reader, ReaderBuilder, StringRecord, Writer, WriterBuilder};
use regex::{Regex, RegexBuilder};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use strsim::normalized_levenshtein;

pub(crate) struct CompiledPattern {
    pub raw: String,
    pub regex: Option<Regex>,
}

pub(crate) fn validate_csv_extension(path: &Path) -> Result<()> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv") => Ok(()),
        _ => Err(CsvCoincidenceError::InvalidExtension(path.to_path_buf())),
    }
}

pub(crate) fn compile_patterns(options: &SearchOptions) -> Result<Vec<CompiledPattern>> {
    if options.patterns.is_empty() {
        return Err(CsvCoincidenceError::Message(
            "at least one pattern is required".into(),
        ));
    }

    let mut compiled = Vec::with_capacity(options.patterns.len());
    for pattern in &options.patterns {
        if pattern.is_empty() {
            return Err(CsvCoincidenceError::Message(
                "pattern must not be empty".into(),
            ));
        }
        let regex = match options.mode {
            MatchMode::Regex => {
                let re = RegexBuilder::new(pattern)
                    .case_insensitive(options.case_insensitive)
                    .build()?;
                Some(re)
            }
            MatchMode::Contains | MatchMode::Fuzzy { .. } => None,
        };
        compiled.push(CompiledPattern {
            raw: pattern.clone(),
            regex,
        });
    }
    Ok(compiled)
}

pub fn open_reader_from_path(path: &Path, options: &SearchOptions) -> Result<Reader<File>> {
    validate_csv_extension(path)?;
    let file = File::open(path)?;
    Ok(build_reader(file, options))
}

pub fn build_reader<R: Read>(reader: R, options: &SearchOptions) -> Reader<R> {
    ReaderBuilder::new()
        .delimiter(options.csv.delimiter)
        .has_headers(options.csv.has_headers)
        .quote(options.csv.quote)
        .trim(if options.csv.trim {
            csv::Trim::All
        } else {
            csv::Trim::None
        })
        .from_reader(reader)
}

fn resolve_column_indices(headers: &StringRecord, selector: &ColumnSelector) -> Result<Vec<usize>> {
    match selector {
        ColumnSelector::All => Ok((0..headers.len()).collect()),
        ColumnSelector::Indices(indices) => {
            for &idx in indices {
                if idx >= headers.len() && !headers.is_empty() {
                    // When headers are disabled, header record length may still reflect first row.
                }
            }
            Ok(indices.clone())
        }
        ColumnSelector::Names(names) => {
            let mut resolved = Vec::new();
            for name in names {
                let idx = headers
                    .iter()
                    .position(|h| h.eq_ignore_ascii_case(name))
                    .ok_or_else(|| CsvCoincidenceError::ColumnNotFound(name.clone()))?;
                resolved.push(idx);
            }
            Ok(resolved)
        }
    }
}

fn field_matches(field: &str, pattern: &CompiledPattern, options: &SearchOptions) -> Option<f64> {
    match &options.mode {
        MatchMode::Regex => {
            let re = pattern.regex.as_ref()?;
            if re.is_match(field) {
                Some(1.0)
            } else {
                None
            }
        }
        MatchMode::Contains => {
            let hay = if options.case_insensitive {
                field.to_lowercase()
            } else {
                field.to_string()
            };
            let needle = if options.case_insensitive {
                pattern.raw.to_lowercase()
            } else {
                pattern.raw.clone()
            };
            if hay.contains(&needle) {
                Some(1.0)
            } else {
                None
            }
        }
        MatchMode::Fuzzy { threshold } => {
            let score = if options.case_insensitive {
                normalized_levenshtein(&field.to_lowercase(), &pattern.raw.to_lowercase())
            } else {
                normalized_levenshtein(field, &pattern.raw)
            };
            if score >= *threshold {
                Some(score)
            } else {
                None
            }
        }
    }
}

fn replace_field(
    field: &str,
    pattern: &CompiledPattern,
    options: &SearchOptions,
    replacement: &str,
) -> Option<String> {
    match &options.mode {
        MatchMode::Regex => {
            let re = pattern.regex.as_ref()?;
            if re.is_match(field) {
                Some(re.replace_all(field, replacement).into_owned())
            } else {
                None
            }
        }
        MatchMode::Contains => {
            if field_matches(field, pattern, options).is_some() {
                if options.case_insensitive {
                    // Case-insensitive replace of all occurrences.
                    let lower = field.to_lowercase();
                    let needle = pattern.raw.to_lowercase();
                    let mut out = String::new();
                    let mut last = 0;
                    let mut start = 0;
                    while let Some(pos) = lower[start..].find(&needle) {
                        let abs = start + pos;
                        out.push_str(&field[last..abs]);
                        out.push_str(replacement);
                        last = abs + needle.len();
                        start = last;
                    }
                    out.push_str(&field[last..]);
                    Some(out)
                } else {
                    Some(field.replace(&pattern.raw, replacement))
                }
            } else {
                None
            }
        }
        MatchMode::Fuzzy { .. } => {
            if field_matches(field, pattern, options).is_some() {
                Some(replacement.to_string())
            } else {
                None
            }
        }
    }
}

/// Stream matches from any CSV reader.
pub fn find_matches_reader<R: Read>(
    mut reader: Reader<R>,
    options: &SearchOptions,
) -> Result<Vec<MatchHit>> {
    let patterns = compile_patterns(options)?;
    let headers = if options.csv.has_headers {
        reader.headers()?.clone()
    } else {
        StringRecord::new()
    };

    // When there are no headers, infer width from the first record lazily.
    let mut column_indices = if options.csv.has_headers {
        resolve_column_indices(&headers, &options.columns)?
    } else {
        match &options.columns {
            ColumnSelector::All => Vec::new(), // filled per-record
            ColumnSelector::Indices(indices) => indices.clone(),
            ColumnSelector::Names(names) => {
                return Err(CsvCoincidenceError::Message(format!(
                    "column names require headers; got {:?}",
                    names
                )));
            }
        }
    };

    let mut hits = Vec::new();
    for (row_idx, result) in reader.records().enumerate() {
        let record = result?;
        if !options.csv.has_headers {
            if let ColumnSelector::All = options.columns {
                column_indices = (0..record.len()).collect();
            }
            for &idx in &column_indices {
                if idx >= record.len() {
                    return Err(CsvCoincidenceError::InvalidColumnIndex(idx));
                }
            }
        } else {
            for &idx in &column_indices {
                if idx >= record.len() {
                    return Err(CsvCoincidenceError::InvalidColumnIndex(idx));
                }
            }
        }

        for &col_idx in &column_indices {
            let value = record.get(col_idx).unwrap_or("");
            for pattern in &patterns {
                if let Some(score) = field_matches(value, pattern, options) {
                    hits.push(MatchHit {
                        row: row_idx + 1,
                        column_index: col_idx,
                        column_name: headers.get(col_idx).map(|s| s.to_string()),
                        value: value.to_string(),
                        pattern: pattern.raw.clone(),
                        score: match options.mode {
                            MatchMode::Fuzzy { .. } => Some(score),
                            _ => None,
                        },
                    });
                }
            }
        }
    }

    Ok(hits)
}

/// Count matching fields.
pub fn count_matches_reader<R: Read>(reader: Reader<R>, options: &SearchOptions) -> Result<usize> {
    Ok(find_matches_reader(reader, options)?.len())
}

/// Keep or drop rows based on whether they contain matches.
pub fn filter_rows_reader<R: Read>(
    mut reader: Reader<R>,
    options: &SearchOptions,
    keep_matches: bool,
) -> Result<String> {
    let patterns = compile_patterns(options)?;
    let headers = if options.csv.has_headers {
        reader.headers()?.clone()
    } else {
        StringRecord::new()
    };

    let mut column_indices = if options.csv.has_headers {
        resolve_column_indices(&headers, &options.columns)?
    } else {
        match &options.columns {
            ColumnSelector::All => Vec::new(),
            ColumnSelector::Indices(indices) => indices.clone(),
            ColumnSelector::Names(_) => {
                return Err(CsvCoincidenceError::Message(
                    "column names require headers".into(),
                ));
            }
        }
    };

    let mut wtr = WriterBuilder::new()
        .delimiter(options.csv.delimiter)
        .quote(options.csv.quote)
        .from_writer(Vec::new());

    if options.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    for result in reader.records() {
        let record = result?;
        if !options.csv.has_headers {
            if let ColumnSelector::All = options.columns {
                column_indices = (0..record.len()).collect();
            }
        }

        let mut row_matches = false;
        for &col_idx in &column_indices {
            if col_idx >= record.len() {
                continue;
            }
            let value = record.get(col_idx).unwrap_or("");
            for pattern in &patterns {
                if field_matches(value, pattern, options).is_some() {
                    row_matches = true;
                    break;
                }
            }
            if row_matches {
                break;
            }
        }

        if row_matches == keep_matches {
            wtr.write_record(&record)?;
        }
    }

    writer_to_string(wtr)
}

/// Replace matching fields and optionally keep only touched rows.
pub fn replace_matches_reader<R: Read>(
    mut reader: Reader<R>,
    options: &ReplaceOptions,
) -> Result<String> {
    let patterns = compile_patterns(&options.search)?;
    let headers = if options.search.csv.has_headers {
        reader.headers()?.clone()
    } else {
        StringRecord::new()
    };

    let mut column_indices = if options.search.csv.has_headers {
        resolve_column_indices(&headers, &options.search.columns)?
    } else {
        match &options.search.columns {
            ColumnSelector::All => Vec::new(),
            ColumnSelector::Indices(indices) => indices.clone(),
            ColumnSelector::Names(_) => {
                return Err(CsvCoincidenceError::Message(
                    "column names require headers".into(),
                ));
            }
        }
    };

    let mut wtr = WriterBuilder::new()
        .delimiter(options.search.csv.delimiter)
        .quote(options.search.csv.quote)
        .from_writer(Vec::new());

    if options.keep_headers && options.search.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    for result in reader.records() {
        let record = result?;
        if !options.search.csv.has_headers {
            if let ColumnSelector::All = options.search.columns {
                column_indices = (0..record.len()).collect();
            }
        }

        let mut out: Vec<String> = record.iter().map(|s| s.to_string()).collect();
        let mut changed = false;

        for &col_idx in &column_indices {
            if col_idx >= out.len() {
                continue;
            }
            let original = out[col_idx].clone();
            for pattern in &patterns {
                if let Some(replaced) =
                    replace_field(&original, pattern, &options.search, &options.replacement)
                {
                    out[col_idx] = replaced;
                    changed = true;
                    break;
                }
            }
        }

        if changed || !options.only_matched_rows {
            wtr.write_record(&out)?;
        }
    }

    writer_to_string(wtr)
}

/// Build a summary report from matches.
pub fn build_report(
    hits: &[MatchHit],
    total_rows: usize,
    total_fields_scanned: usize,
) -> ScanReport {
    let mut pattern_map: HashMap<&str, usize> = HashMap::new();
    let mut value_map: HashMap<&str, usize> = HashMap::new();
    let mut columns: HashSet<String> = HashSet::new();
    let mut matched_rows: HashSet<usize> = HashSet::new();

    for hit in hits {
        *pattern_map.entry(hit.pattern.as_str()).or_insert(0) += 1;
        *value_map.entry(hit.value.as_str()).or_insert(0) += 1;
        matched_rows.insert(hit.row);
        if let Some(name) = &hit.column_name {
            columns.insert(name.clone());
        } else {
            columns.insert(format!("col_{}", hit.column_index));
        }
    }

    let mut pattern_counts: Vec<PatternCount> = pattern_map
        .into_iter()
        .map(|(pattern, count)| PatternCount {
            pattern: pattern.to_string(),
            count,
        })
        .collect();
    pattern_counts.sort_by(|a, b| b.count.cmp(&a.count).then(a.pattern.cmp(&b.pattern)));

    let mut top_values: Vec<ValueCount> = value_map
        .into_iter()
        .map(|(value, count)| ValueCount {
            value: value.to_string(),
            count,
        })
        .collect();
    top_values.sort_by(|a, b| b.count.cmp(&a.count).then(a.value.cmp(&b.value)));
    top_values.truncate(10);

    let mut columns_touched: Vec<String> = columns.into_iter().collect();
    columns_touched.sort();

    ScanReport {
        total_rows,
        total_fields_scanned,
        total_matches: hits.len(),
        matched_rows: matched_rows.len(),
        columns_touched,
        pattern_counts,
        top_values,
    }
}

/// Count rows and fields while scanning (for report metadata).
pub fn scan_stats_reader<R: Read>(
    mut reader: Reader<R>,
    options: &SearchOptions,
) -> Result<(Vec<MatchHit>, usize, usize)> {
    let patterns = compile_patterns(options)?;
    let headers = if options.csv.has_headers {
        reader.headers()?.clone()
    } else {
        StringRecord::new()
    };

    let mut column_indices = if options.csv.has_headers {
        resolve_column_indices(&headers, &options.columns)?
    } else {
        match &options.columns {
            ColumnSelector::All => Vec::new(),
            ColumnSelector::Indices(indices) => indices.clone(),
            ColumnSelector::Names(_) => {
                return Err(CsvCoincidenceError::Message(
                    "column names require headers".into(),
                ));
            }
        }
    };

    let mut hits = Vec::new();
    let mut total_rows = 0usize;
    let mut total_fields = 0usize;

    for (row_idx, result) in reader.records().enumerate() {
        let record = result?;
        total_rows += 1;
        if !options.csv.has_headers {
            if let ColumnSelector::All = options.columns {
                column_indices = (0..record.len()).collect();
            }
        }

        for &col_idx in &column_indices {
            if col_idx >= record.len() {
                continue;
            }
            total_fields += 1;
            let value = record.get(col_idx).unwrap_or("");
            for pattern in &patterns {
                if let Some(score) = field_matches(value, pattern, options) {
                    hits.push(MatchHit {
                        row: row_idx + 1,
                        column_index: col_idx,
                        column_name: headers.get(col_idx).map(|s| s.to_string()),
                        value: value.to_string(),
                        pattern: pattern.raw.clone(),
                        score: match options.mode {
                            MatchMode::Fuzzy { .. } => Some(score),
                            _ => None,
                        },
                    });
                }
            }
        }
    }

    Ok((hits, total_rows, total_fields))
}

/// Deduplicate rows that share the same fingerprint of matched column values.
pub fn dedupe_rows_reader<R: Read>(
    mut reader: Reader<R>,
    options: &SearchOptions,
) -> Result<String> {
    let patterns = compile_patterns(options)?;
    let headers = if options.csv.has_headers {
        reader.headers()?.clone()
    } else {
        StringRecord::new()
    };

    let mut column_indices = if options.csv.has_headers {
        resolve_column_indices(&headers, &options.columns)?
    } else {
        match &options.columns {
            ColumnSelector::All => Vec::new(),
            ColumnSelector::Indices(indices) => indices.clone(),
            ColumnSelector::Names(_) => {
                return Err(CsvCoincidenceError::Message(
                    "column names require headers".into(),
                ));
            }
        }
    };

    let mut wtr = WriterBuilder::new()
        .delimiter(options.csv.delimiter)
        .quote(options.csv.quote)
        .from_writer(Vec::new());

    if options.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    let mut seen: HashSet<String> = HashSet::new();

    for result in reader.records() {
        let record = result?;
        if !options.csv.has_headers {
            if let ColumnSelector::All = options.columns {
                column_indices = (0..record.len()).collect();
            }
        }

        let mut key_parts = Vec::new();
        let mut any_match = false;
        for &col_idx in &column_indices {
            let value = record.get(col_idx).unwrap_or("");
            for pattern in &patterns {
                if field_matches(value, pattern, options).is_some() {
                    any_match = true;
                    key_parts.push(format!("{col_idx}:{value}"));
                    break;
                }
            }
        }

        let key = if any_match {
            key_parts.join("|")
        } else {
            record.iter().collect::<Vec<_>>().join("\u{1f}")
        };

        if seen.insert(key) {
            wtr.write_record(&record)?;
        }
    }

    writer_to_string(wtr)
}

pub fn write_output(path: Option<&Path>, content: &str) -> Result<()> {
    match path {
        Some(path) => {
            let mut file = File::create(path)?;
            file.write_all(content.as_bytes())?;
        }
        None => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(content.as_bytes())?;
        }
    }
    Ok(())
}

pub(crate) fn writer_to_string(wtr: Writer<Vec<u8>>) -> Result<String> {
    let bytes = wtr.into_inner().map_err(|err| {
        CsvCoincidenceError::Message(format!("failed to finalize CSV writer: {err}"))
    })?;
    Ok(String::from_utf8(bytes)?)
}
