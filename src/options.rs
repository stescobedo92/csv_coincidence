/// Dialect and parsing options for CSV input.
#[derive(Debug, Clone)]
pub struct CsvOptions {
    /// Field delimiter (default: `,`).
    pub delimiter: u8,
    /// Whether the first row contains headers (default: `true`).
    pub has_headers: bool,
    /// Quote character (default: `"`).
    pub quote: u8,
    /// Soft-trim field whitespace before matching.
    pub trim: bool,
}

impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            has_headers: true,
            quote: b'"',
            trim: false,
        }
    }
}

impl CsvOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn delimiter(mut self, delimiter: u8) -> Self {
        self.delimiter = delimiter;
        self
    }

    pub fn has_headers(mut self, has_headers: bool) -> Self {
        self.has_headers = has_headers;
        self
    }

    pub fn quote(mut self, quote: u8) -> Self {
        self.quote = quote;
        self
    }

    pub fn trim(mut self, trim: bool) -> Self {
        self.trim = trim;
        self
    }
}

/// How to select columns for matching.
#[derive(Debug, Clone, Default)]
pub enum ColumnSelector {
    /// Search every field.
    #[default]
    All,
    /// Search by header name(s).
    Names(Vec<String>),
    /// Search by zero-based column indices.
    Indices(Vec<usize>),
}

/// Matching strategy.
#[derive(Debug, Clone, Default)]
pub enum MatchMode {
    /// Regular expression match.
    #[default]
    Regex,
    /// Substring contains (case-sensitive).
    Contains,
    /// Fuzzy similarity using normalized Levenshtein (0.0–1.0).
    Fuzzy { threshold: f64 },
}

/// Search / transform configuration.
#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub csv: CsvOptions,
    pub columns: ColumnSelector,
    pub mode: MatchMode,
    /// One or more patterns (regex, substring, or fuzzy target).
    pub patterns: Vec<String>,
    /// Case-insensitive matching (regex / contains).
    pub case_insensitive: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            csv: CsvOptions::default(),
            columns: ColumnSelector::All,
            mode: MatchMode::Regex,
            patterns: Vec::new(),
            case_insensitive: false,
        }
    }
}

impl SearchOptions {
    pub fn new(patterns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            patterns: patterns.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    pub fn csv(mut self, csv: CsvOptions) -> Self {
        self.csv = csv;
        self
    }

    pub fn columns(mut self, columns: ColumnSelector) -> Self {
        self.columns = columns;
        self
    }

    pub fn mode(mut self, mode: MatchMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn case_insensitive(mut self, value: bool) -> Self {
        self.case_insensitive = value;
        self
    }
}

/// Replacement configuration for merge / replace operations.
#[derive(Debug, Clone)]
pub struct ReplaceOptions {
    pub search: SearchOptions,
    /// Replacement text. Supports `$0`, `$1`, … capture groups for regex mode.
    pub replacement: String,
    /// Keep only rows that had at least one replacement.
    pub only_matched_rows: bool,
    /// Keep the header row in the output.
    pub keep_headers: bool,
}

impl ReplaceOptions {
    pub fn new(
        patterns: impl IntoIterator<Item = impl Into<String>>,
        replacement: impl Into<String>,
    ) -> Self {
        Self {
            search: SearchOptions::new(patterns),
            replacement: replacement.into(),
            only_matched_rows: true,
            keep_headers: true,
        }
    }

    pub fn only_matched_rows(mut self, value: bool) -> Self {
        self.only_matched_rows = value;
        self
    }

    pub fn keep_headers(mut self, value: bool) -> Self {
        self.keep_headers = value;
        self
    }

    pub fn search(mut self, search: SearchOptions) -> Self {
        self.search = search;
        self
    }
}
