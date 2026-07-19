use serde::{Deserialize, Serialize};

/// A single field-level match with row/column context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchHit {
    /// 1-based data row number (header is not counted).
    pub row: usize,
    /// Zero-based column index.
    pub column_index: usize,
    /// Header name when available.
    pub column_name: Option<String>,
    /// Original field value.
    pub value: String,
    /// Pattern that produced the hit.
    pub pattern: String,
    /// Optional similarity score for fuzzy matches (0.0–1.0).
    pub score: Option<f64>,
}

/// Aggregate report for a scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScanReport {
    pub total_rows: usize,
    pub total_fields_scanned: usize,
    pub total_matches: usize,
    pub matched_rows: usize,
    pub columns_touched: Vec<String>,
    pub pattern_counts: Vec<PatternCount>,
    pub top_values: Vec<ValueCount>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternCount {
    pub pattern: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueCount {
    pub value: String,
    pub count: usize,
}
