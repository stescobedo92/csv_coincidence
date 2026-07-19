use clap::{Parser, Subcommand, ValueEnum};
use csv_coincidence::{
    count_matches, dedupe_rows, export_hits, export_report, filter_rows, find_matches,
    find_matches_from_read, replace_matches, report_matches, write_output, ColumnSelector,
    CsvOptions, ExportFormat, MatchMode, ReplaceOptions, SearchOptions,
};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, ValueEnum)]
enum CliFormat {
    Text,
    Csv,
    Tsv,
    Json,
}

impl From<CliFormat> for ExportFormat {
    fn from(value: CliFormat) -> Self {
        match value {
            CliFormat::Text => ExportFormat::Text,
            CliFormat::Csv => ExportFormat::Csv,
            CliFormat::Tsv => ExportFormat::Tsv,
            CliFormat::Json => ExportFormat::Json,
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
enum CliMode {
    Regex,
    Contains,
    Fuzzy,
}

#[derive(Parser, Debug)]
#[command(
    name = "csv-coincidence",
    version,
    about = "Search, count, filter, replace, and report pattern matches in CSV files"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Input CSV path. Use `-` for stdin.
    #[arg(global = true, short, long)]
    input: Option<PathBuf>,

    /// Output path. Defaults to stdout.
    #[arg(global = true, short, long)]
    output: Option<PathBuf>,

    /// One or more patterns (repeatable).
    #[arg(global = true, short = 'p', long = "pattern")]
    patterns: Vec<String>,

    /// Restrict matching to column names (repeatable).
    #[arg(global = true, short = 'c', long = "column")]
    columns: Vec<String>,

    /// Restrict matching to zero-based column indices (repeatable).
    #[arg(global = true, long = "column-index")]
    column_indices: Vec<usize>,

    /// Field delimiter (default `,`, use `\\t` for tab).
    #[arg(global = true, short = 'd', long, default_value = ",")]
    delimiter: String,

    /// Treat the first row as headers.
    #[arg(global = true, long, default_value_t = true, action = clap::ArgAction::Set)]
    headers: bool,

    /// Trim whitespace in fields before matching.
    #[arg(global = true, long, default_value_t = false)]
    trim: bool,

    /// Case-insensitive matching.
    #[arg(
        global = true,
        short = 'I',
        long = "ignore-case",
        default_value_t = false
    )]
    ignore_case: bool,

    /// Matching mode.
    #[arg(global = true, long, value_enum, default_value_t = CliMode::Regex)]
    mode: CliMode,

    /// Fuzzy similarity threshold (0.0–1.0).
    #[arg(global = true, long, default_value_t = 0.8)]
    fuzzy_threshold: f64,

    /// Output format for structured commands.
    #[arg(global = true, short = 'f', long, value_enum, default_value_t = CliFormat::Text)]
    format: CliFormat,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Find matching fields with row/column context
    Find,
    /// Count matching fields
    Count,
    /// Keep rows that match
    Filter {
        /// Invert: keep rows that do not match
        #[arg(long, default_value_t = false)]
        invert: bool,
    },
    /// Replace matching fields
    Replace {
        /// Replacement text (`$0`, `$1`, … for regex captures)
        #[arg(short = 'r', long, default_value = "[MERGED]")]
        replacement: String,
        /// Keep only rows that were changed
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        only_matched_rows: bool,
        /// Preserve header row in output
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        keep_headers: bool,
    },
    /// Compatibility alias for replace with `[MERGED]`
    Merge,
    /// Summary report of matches
    Report,
    /// Deduplicate rows using matched-column fingerprints
    Dedupe,
}

fn parse_delimiter(raw: &str) -> Result<u8, String> {
    match raw {
        r"\t" | "tab" | "\t" => Ok(b'\t'),
        s if s.len() == 1 => Ok(s.as_bytes()[0]),
        _ => Err(format!(
            "delimiter must be a single character or \\t, got {raw:?}"
        )),
    }
}

fn build_options(cli: &Cli) -> Result<SearchOptions, String> {
    if cli.patterns.is_empty() {
        return Err("at least one --pattern is required".into());
    }

    let delimiter = parse_delimiter(&cli.delimiter)?;
    let csv = CsvOptions::new()
        .delimiter(delimiter)
        .has_headers(cli.headers)
        .trim(cli.trim);

    let columns = if !cli.columns.is_empty() {
        ColumnSelector::Names(cli.columns.clone())
    } else if !cli.column_indices.is_empty() {
        ColumnSelector::Indices(cli.column_indices.clone())
    } else {
        ColumnSelector::All
    };

    let mode = match cli.mode {
        CliMode::Regex => MatchMode::Regex,
        CliMode::Contains => MatchMode::Contains,
        CliMode::Fuzzy => MatchMode::Fuzzy {
            threshold: cli.fuzzy_threshold,
        },
    };

    Ok(SearchOptions::new(cli.patterns.clone())
        .csv(csv)
        .columns(columns)
        .mode(mode)
        .case_insensitive(cli.ignore_case))
}

fn read_stdin() -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let options = build_options(&cli).map_err(|e| e.to_string())?;
    let format = ExportFormat::from(cli.format.clone());

    let input = cli.input.clone().unwrap_or_else(|| PathBuf::from("-"));
    let from_stdin = input == Path::new("-");

    let output = |content: &str| write_output(cli.output.as_deref(), content);

    match cli.command {
        Commands::Find => {
            let hits = if from_stdin {
                find_matches_from_read(&read_stdin()?[..], options)?
            } else {
                find_matches(&input, options)?
            };
            output(&export_hits(&hits, format)?)?;
        }
        Commands::Count => {
            let count = if from_stdin {
                find_matches_from_read(&read_stdin()?[..], options)?.len()
            } else {
                count_matches(&input, options)?
            };
            output(&format!("{count}\n"))?;
        }
        Commands::Filter { invert } => {
            let csv_out = if from_stdin {
                let data = read_stdin()?;
                let reader = csv_coincidence::build_reader(&data[..], &options);
                csv_coincidence::filter_rows_reader(reader, &options, !invert)?
            } else {
                filter_rows(&input, options, !invert)?
            };
            output(&csv_out)?;
        }
        Commands::Replace {
            replacement,
            only_matched_rows,
            keep_headers,
        } => {
            let replace = ReplaceOptions {
                search: options,
                replacement,
                only_matched_rows,
                keep_headers,
            };
            let csv_out = if from_stdin {
                let data = read_stdin()?;
                let reader = csv_coincidence::build_reader(&data[..], &replace.search);
                csv_coincidence::replace_matches_reader(reader, &replace)?
            } else {
                replace_matches(&input, replace)?
            };
            output(&csv_out)?;
        }
        Commands::Merge => {
            let replace = ReplaceOptions::new(cli.patterns.clone(), "[MERGED]")
                .search(options)
                .only_matched_rows(true)
                .keep_headers(false);
            let csv_out = if from_stdin {
                let data = read_stdin()?;
                let reader = csv_coincidence::build_reader(&data[..], &replace.search);
                csv_coincidence::replace_matches_reader(reader, &replace)?
            } else {
                replace_matches(&input, replace)?
            };
            output(&csv_out)?;
        }
        Commands::Report => {
            let report = if from_stdin {
                let data = read_stdin()?;
                let reader = csv_coincidence::build_reader(&data[..], &options);
                let (hits, rows, fields) = csv_coincidence::scan_stats_reader(reader, &options)?;
                csv_coincidence::build_report(&hits, rows, fields)
            } else {
                report_matches(&input, options)?
            };
            output(&export_report(&report, format)?)?;
        }
        Commands::Dedupe => {
            let csv_out = if from_stdin {
                let data = read_stdin()?;
                let reader = csv_coincidence::build_reader(&data[..], &options);
                csv_coincidence::dedupe_rows_reader(reader, &options)?
            } else {
                dedupe_rows(&input, options)?
            };
            output(&csv_out)?;
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
