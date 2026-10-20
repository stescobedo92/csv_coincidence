use crate::error::{CsvCoincidenceError, Result};
use crate::match_result::{MatchHit, PatternCount, ScanReport, ValueCount};
use crate::options::{
    ColumnRule, ColumnSelector, LogicOp, MatchMode, ReplaceOptions, SearchOptions,
};
use csv::{Reader, ReaderBuilder, StringRecord, Writer, WriterBuilder};
use regex::{Regex, RegexBuilder};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use strsim::normalized_levenshtein;

struct CompiledPattern {
    raw: String,
    regex: Option<Regex>,
}

struct FieldMatch {
    score: f64,
    start: usize,
    end: usize,
    matched_text: String,
    captures: Vec<Option<String>>,
}

enum ColumnIndices {
    All,
    Fixed(Vec<usize>),
}

struct CompiledRule {
    indices: ColumnIndices,
    patterns: Vec<CompiledPattern>,
    mode: MatchMode,
    case_insensitive: bool,
    negated: bool,
}

impl CompiledRule {
    fn indices_for(&self, record_len: usize) -> Vec<usize> {
        match &self.indices {
            ColumnIndices::All => (0..record_len).collect(),
            ColumnIndices::Fixed(indices) => indices.clone(),
        }
    }
}

pub(crate) fn validate_csv_extension(path: &Path) -> Result<()> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") || ext.eq_ignore_ascii_case("tsv") => Ok(()),
        _ => Err(CsvCoincidenceError::InvalidExtension(path.to_path_buf())),
    }
}

fn validate_mode(mode: &MatchMode) -> Result<()> {
    if let MatchMode::Fuzzy { threshold } = mode {
        if !(0.0..=1.0).contains(threshold) {
            return Err(CsvCoincidenceError::Message(format!(
                "fuzzy threshold must be between 0.0 and 1.0, got {threshold}"
            )));
        }
    }
    Ok(())
}

/// Normalise options into rules: explicit `column_rules`, or one legacy
/// rule built from the global fields.
fn effective_rules(options: &SearchOptions) -> Vec<ColumnRule> {
    if options.column_rules.is_empty() {
        vec![ColumnRule {
            columns: options.columns.clone(),
            patterns: options.patterns.clone(),
            mode: options.mode.clone(),
            case_insensitive: options.case_insensitive,
            negated: false,
        }]
    } else {
        options.column_rules.clone()
    }
}

fn compile_rule(
    rule: &ColumnRule,
    headers: &StringRecord,
    has_headers: bool,
) -> Result<CompiledRule> {
    if rule.patterns.is_empty() {
        return Err(CsvCoincidenceError::Message(
            "at least one pattern is required".into(),
        ));
    }
    validate_mode(&rule.mode)?;

    let mut patterns = Vec::with_capacity(rule.patterns.len());
    for pattern in &rule.patterns {
        if pattern.is_empty() {
            return Err(CsvCoincidenceError::Message(
                "pattern must not be empty".into(),
            ));
        }
        let regex = match rule.mode {
            MatchMode::Regex => Some(
                RegexBuilder::new(pattern)
                    .case_insensitive(rule.case_insensitive)
                    .build()?,
            ),
            MatchMode::Contains | MatchMode::Fuzzy { .. } => None,
        };
        patterns.push(CompiledPattern {
            raw: pattern.clone(),
            regex,
        });
    }

    let indices = if has_headers {
        ColumnIndices::Fixed(resolve_column_indices(headers, &rule.columns)?)
    } else {
        match &rule.columns {
            ColumnSelector::All => ColumnIndices::All,
            ColumnSelector::Indices(indices) => ColumnIndices::Fixed(indices.clone()),
            ColumnSelector::Names(names) => {
                return Err(CsvCoincidenceError::Message(format!(
                    "column names require headers; got {names:?}"
                )));
            }
        }
    };

    Ok(CompiledRule {
        indices,
        patterns,
        mode: rule.mode.clone(),
        case_insensitive: rule.case_insensitive,
        negated: rule.negated,
    })
}

fn compile_rules(options: &SearchOptions, headers: &StringRecord) -> Result<Vec<CompiledRule>> {
    effective_rules(options)
        .iter()
        .map(|rule| compile_rule(rule, headers, options.csv.has_headers))
        .collect()
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

fn read_headers<R: Read>(reader: &mut Reader<R>, options: &SearchOptions) -> Result<StringRecord> {
    if options.csv.has_headers {
        Ok(reader.headers()?.clone())
    } else {
        Ok(StringRecord::new())
    }
}

fn build_writer<W: Write>(writer: W, options: &SearchOptions) -> Writer<W> {
    WriterBuilder::new()
        .delimiter(options.csv.delimiter)
        .quote(options.csv.quote)
        .from_writer(writer)
}

fn resolve_column_indices(headers: &StringRecord, selector: &ColumnSelector) -> Result<Vec<usize>> {
    match selector {
        ColumnSelector::All => Ok((0..headers.len()).collect()),
        ColumnSelector::Indices(indices) => Ok(indices.clone()),
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

fn field_match(
    field: &str,
    pattern: &CompiledPattern,
    mode: &MatchMode,
    case_insensitive: bool,
) -> Option<FieldMatch> {
    match mode {
        MatchMode::Regex => {
            let re = pattern.regex.as_ref()?;
            let captures = re.captures(field)?;
            let whole = captures.get(0)?;
            Some(FieldMatch {
                score: 1.0,
                start: whole.start(),
                end: whole.end(),
                matched_text: whole.as_str().to_string(),
                captures: captures
                    .iter()
                    .skip(1)
                    .map(|group| group.map(|m| m.as_str().to_string()))
                    .collect(),
            })
        }
        MatchMode::Contains => {
            let (haystack, needle) = if case_insensitive {
                (field.to_lowercase(), pattern.raw.to_lowercase())
            } else {
                (field.to_string(), pattern.raw.clone())
            };
            let start = haystack.find(&needle)?;
            let end = start + needle.len();
            let matched_text = if field.is_char_boundary(start) && field.is_char_boundary(end) {
                field[start..end].to_string()
            } else {
                field.to_string()
            };
            Some(FieldMatch {
                score: 1.0,
                start,
                end,
                matched_text,
                captures: Vec::new(),
            })
        }
        MatchMode::Fuzzy { threshold } => {
            let score = if case_insensitive {
                normalized_levenshtein(&field.to_lowercase(), &pattern.raw.to_lowercase())
            } else {
                normalized_levenshtein(field, &pattern.raw)
            };
            if score >= *threshold {
                Some(FieldMatch {
                    score,
                    start: 0,
                    end: field.len(),
                    matched_text: field.to_string(),
                    captures: Vec::new(),
                })
            } else {
                None
            }
        }
    }
}

fn replace_field(
    field: &str,
    pattern: &CompiledPattern,
    mode: &MatchMode,
    case_insensitive: bool,
    replacement: &str,
) -> Option<String> {
    match mode {
        MatchMode::Regex => {
            let re = pattern.regex.as_ref()?;
            if re.is_match(field) {
                Some(re.replace_all(field, replacement).into_owned())
            } else {
                None
            }
        }
        MatchMode::Contains => {
            field_match(field, pattern, mode, case_insensitive)?;
            if case_insensitive {
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
        }
        MatchMode::Fuzzy { .. } => {
            if field_match(field, pattern, mode, case_insensitive).is_some() {
                Some(replacement.to_string())
            } else {
                None
            }
        }
    }
}

fn combine(satisfied: &[bool], logic: LogicOp) -> bool {
    match logic {
        LogicOp::And => satisfied.iter().all(|&s| s),
        LogicOp::Or => satisfied.iter().any(|&s| s),
    }
}

/// Hits from non-negated rules, but only when the combined row condition
/// holds. With `strict`, an out-of-range fixed column index is an error.
fn evaluate_row_hits(
    record: &StringRecord,
    headers: &StringRecord,
    row: usize,
    rules: &[CompiledRule],
    logic: LogicOp,
    strict: bool,
) -> Result<Vec<MatchHit>> {
    let mut satisfied = Vec::with_capacity(rules.len());
    let mut hits = Vec::new();

    for rule in rules {
        let mut any_match = false;
        for col_idx in rule.indices_for(record.len()) {
            if col_idx >= record.len() {
                if strict {
                    return Err(CsvCoincidenceError::InvalidColumnIndex(col_idx));
                }
                continue;
            }
            let value = record.get(col_idx).unwrap_or("");
            for pattern in &rule.patterns {
                if let Some(found) = field_match(value, pattern, &rule.mode, rule.case_insensitive)
                {
                    any_match = true;
                    if !rule.negated {
                        hits.push(MatchHit {
                            row,
                            column_index: col_idx,
                            column_name: headers.get(col_idx).map(str::to_string),
                            value: value.to_string(),
                            pattern: pattern.raw.clone(),
                            score: match rule.mode {
                                MatchMode::Fuzzy { .. } => Some(found.score),
                                _ => None,
                            },
                            matched_text: found.matched_text,
                            start: found.start,
                            end: found.end,
                            captures: found.captures,
                        });
                    }
                }
            }
        }
        satisfied.push(any_match != rule.negated);
    }

    if combine(&satisfied, logic) {
        Ok(hits)
    } else {
        Ok(Vec::new())
    }
}

/// Row-level boolean evaluation without building hits.
fn evaluate_row_bool(record: &StringRecord, rules: &[CompiledRule], logic: LogicOp) -> bool {
    let mut satisfied = Vec::with_capacity(rules.len());
    for rule in rules {
        let mut any_match = false;
        for col_idx in rule.indices_for(record.len()) {
            let Some(value) = record.get(col_idx) else {
                continue;
            };
            if rule
                .patterns
                .iter()
                .any(|p| field_match(value, p, &rule.mode, rule.case_insensitive).is_some())
            {
                any_match = true;
                break;
            }
        }
        satisfied.push(any_match != rule.negated);
    }
    combine(&satisfied, logic)
}

/// Streaming iterator over matches, reading one CSV record at a time.
pub struct MatchHitIter<R: Read> {
    reader: Reader<R>,
    headers: StringRecord,
    rules: Vec<CompiledRule>,
    logic: LogicOp,
    row: usize,
    pending: VecDeque<MatchHit>,
    exhausted: bool,
}

/// Create a streaming match iterator from any CSV reader.
///
/// Patterns, rules and column names are validated up front; row-level
/// errors (malformed CSV, invalid column index) surface as iterator items.
pub fn find_matches_iter_reader<R: Read>(
    mut reader: Reader<R>,
    options: &SearchOptions,
) -> Result<MatchHitIter<R>> {
    let headers = read_headers(&mut reader, options)?;
    let rules = compile_rules(options, &headers)?;
    Ok(MatchHitIter {
        reader,
        headers,
        rules,
        logic: options.logic,
        row: 0,
        pending: VecDeque::new(),
        exhausted: false,
    })
}

impl<R: Read> Iterator for MatchHitIter<R> {
    type Item = Result<MatchHit>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(hit) = self.pending.pop_front() {
                return Some(Ok(hit));
            }
            if self.exhausted {
                return None;
            }
            let mut record = StringRecord::new();
            match self.reader.read_record(&mut record) {
                Ok(true) => {
                    self.row += 1;
                    match evaluate_row_hits(
                        &record,
                        &self.headers,
                        self.row,
                        &self.rules,
                        self.logic,
                        true,
                    ) {
                        Ok(hits) => self.pending.extend(hits),
                        Err(err) => {
                            self.exhausted = true;
                            return Some(Err(err));
                        }
                    }
                }
                Ok(false) => self.exhausted = true,
                Err(err) => {
                    self.exhausted = true;
                    return Some(Err(err.into()));
                }
            }
        }
    }
}

/// Stream matches from any CSV reader.
pub fn find_matches_reader<R: Read>(
    reader: Reader<R>,
    options: &SearchOptions,
) -> Result<Vec<MatchHit>> {
    find_matches_iter_reader(reader, options)?.collect()
}

/// Count matching fields without materialising the hits.
pub fn count_matches_reader<R: Read>(reader: Reader<R>, options: &SearchOptions) -> Result<usize> {
    let mut count = 0usize;
    for hit in find_matches_iter_reader(reader, options)? {
        hit?;
        count += 1;
    }
    Ok(count)
}

/// Keep or drop rows based on matches, writing to `writer`.
/// Returns the number of data rows written.
pub fn filter_rows_to_writer<R: Read, W: Write>(
    mut reader: Reader<R>,
    options: &SearchOptions,
    keep_matches: bool,
    writer: W,
) -> Result<usize> {
    let headers = read_headers(&mut reader, options)?;
    let rules = compile_rules(options, &headers)?;
    let mut wtr = build_writer(writer, options);

    if options.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    let mut written = 0usize;
    for result in reader.records() {
        let record = result?;
        if evaluate_row_bool(&record, &rules, options.logic) == keep_matches {
            wtr.write_record(&record)?;
            written += 1;
        }
    }
    wtr.flush()?;
    Ok(written)
}

/// Keep or drop rows based on whether they contain matches.
pub fn filter_rows_reader<R: Read>(
    reader: Reader<R>,
    options: &SearchOptions,
    keep_matches: bool,
) -> Result<String> {
    let mut buffer = Vec::new();
    filter_rows_to_writer(reader, options, keep_matches, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}

/// Replace matching fields, writing to `writer`.
/// Returns the number of data rows written.
pub fn replace_matches_to_writer<R: Read, W: Write>(
    mut reader: Reader<R>,
    options: &ReplaceOptions,
    writer: W,
) -> Result<usize> {
    let headers = read_headers(&mut reader, &options.search)?;
    let rules = compile_rules(&options.search, &headers)?;
    let mut wtr = build_writer(writer, &options.search);

    if options.keep_headers && options.search.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    let mut written = 0usize;
    for result in reader.records() {
        let record = result?;
        let mut out: Vec<String> = record.iter().map(str::to_string).collect();
        let mut changed = false;

        for rule in rules.iter().filter(|rule| !rule.negated) {
            for col_idx in rule.indices_for(out.len()) {
                if col_idx >= out.len() {
                    continue;
                }
                let original = out[col_idx].clone();
                for pattern in &rule.patterns {
                    if let Some(replaced) = replace_field(
                        &original,
                        pattern,
                        &rule.mode,
                        rule.case_insensitive,
                        &options.replacement,
                    ) {
                        out[col_idx] = replaced;
                        changed = true;
                        break;
                    }
                }
            }
        }

        if changed || !options.only_matched_rows {
            wtr.write_record(&out)?;
            written += 1;
        }
    }
    wtr.flush()?;
    Ok(written)
}

/// Replace matching fields and optionally keep only touched rows.
pub fn replace_matches_reader<R: Read>(
    reader: Reader<R>,
    options: &ReplaceOptions,
) -> Result<String> {
    let mut buffer = Vec::new();
    replace_matches_to_writer(reader, options, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
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
    let headers = read_headers(&mut reader, options)?;
    let rules = compile_rules(options, &headers)?;

    let mut hits = Vec::new();
    let mut total_rows = 0usize;
    let mut total_fields = 0usize;

    let mut record = StringRecord::new();
    while reader.read_record(&mut record)? {
        total_rows += 1;
        for rule in &rules {
            total_fields += rule
                .indices_for(record.len())
                .iter()
                .filter(|&&idx| idx < record.len())
                .count();
        }
        hits.extend(evaluate_row_hits(
            &record,
            &headers,
            total_rows,
            &rules,
            options.logic,
            false,
        )?);
    }

    Ok((hits, total_rows, total_fields))
}

/// Deduplicate rows, writing to `writer`.
/// Returns the number of data rows written.
pub fn dedupe_rows_to_writer<R: Read, W: Write>(
    mut reader: Reader<R>,
    options: &SearchOptions,
    writer: W,
) -> Result<usize> {
    let headers = read_headers(&mut reader, options)?;
    let rules = compile_rules(options, &headers)?;
    let mut wtr = build_writer(writer, options);

    if options.csv.has_headers {
        wtr.write_record(&headers)?;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut written = 0usize;

    for result in reader.records() {
        let record = result?;
        let mut key_parts = Vec::new();
        let mut seen_columns: HashSet<usize> = HashSet::new();
        for rule in rules.iter().filter(|rule| !rule.negated) {
            for col_idx in rule.indices_for(record.len()) {
                if seen_columns.contains(&col_idx) {
                    continue;
                }
                let Some(value) = record.get(col_idx) else {
                    continue;
                };
                if rule.patterns.iter().any(|pattern| {
                    field_match(value, pattern, &rule.mode, rule.case_insensitive).is_some()
                }) {
                    seen_columns.insert(col_idx);
                    key_parts.push(format!("{col_idx}:{value}"));
                }
            }
        }

        let key = if key_parts.is_empty() {
            record.iter().collect::<Vec<_>>().join("\u{1f}")
        } else {
            key_parts.join("|")
        };

        if seen.insert(key) {
            wtr.write_record(&record)?;
            written += 1;
        }
    }
    wtr.flush()?;
    Ok(written)
}

/// Deduplicate rows that share the same fingerprint of matched column values.
pub fn dedupe_rows_reader<R: Read>(reader: Reader<R>, options: &SearchOptions) -> Result<String> {
    let mut buffer = Vec::new();
    dedupe_rows_to_writer(reader, options, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
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
