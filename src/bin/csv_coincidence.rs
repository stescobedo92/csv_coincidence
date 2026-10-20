use clap::{Parser, Subcommand, ValueEnum};
use csv_coincidence::{
    build_reader, build_report, builtin_pattern, builtin_patterns, count_matches,
    dedupe_rows_to_writer, export_hits, export_report, filter_rows_to_writer, find_matches,
    find_matches_from_read, find_matches_iter_from_read, open_reader_from_path, read_pattern_file,
    replace_matches_to_writer, report_matches, scan_stats_reader, write_output, ColumnRule,
    ColumnSelector, CsvOptions, ExportFormat, LogicOp, MatchMode, ReplaceOptions, SearchOptions,
};
use std::fs::File;
use std::io::{self, Read, Write};
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

#[derive(Debug, Clone, ValueEnum)]
enum CliLogic {
    And,
    Or,
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

    /// Read patterns from a file, one per line (`#` comments allowed).
    #[arg(global = true, long = "pattern-file")]
    pattern_file: Option<PathBuf>,

    /// Add a built-in pattern (repeatable): email, url, ipv4, iban,
    /// credit-card, dni, nie, phone-es, date-iso, uuid.
    #[arg(global = true, long = "builtin")]
    builtins: Vec<String>,

    /// Per-column rule `[!]column=pattern` (repeatable). A leading `!`
    /// negates the rule. Combined with `--logic`.
    #[arg(global = true, long = "where")]
    wheres: Vec<String>,

    /// How `--where` rules combine.
    #[arg(global = true, long, value_enum, default_value_t = CliLogic::And)]
    logic: CliLogic,

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

fn parse_where(raw: &str, mode: &MatchMode, case_insensitive: bool) -> Result<ColumnRule, String> {
    let (negated, body) = match raw.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let (column, pattern) = body
        .split_once('=')
        .filter(|(column, pattern)| !column.trim().is_empty() && !pattern.is_empty())
        .ok_or_else(|| format!("--where must be [!]column=pattern, got {raw:?}"))?;
    Ok(ColumnRule::new(
        ColumnSelector::Names(vec![column.trim().to_string()]),
        [pattern.to_string()],
    )
    .mode(mode.clone())
    .case_insensitive(case_insensitive)
    .negated(negated))
}

fn collect_patterns(cli: &Cli) -> Result<Vec<String>, String> {
    let mut patterns = cli.patterns.clone();
    for name in &cli.builtins {
        match builtin_pattern(name) {
            Some(pattern) => patterns.push(pattern.to_string()),
            None => {
                let available = builtin_patterns()
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(format!("unknown builtin {name:?}; available: {available}"));
            }
        }
    }
    if let Some(path) = &cli.pattern_file {
        if path.as_os_str() == "-" {
            return Err("--pattern-file - conflicts with stdin input".into());
        }
        patterns.extend(read_pattern_file(path).map_err(|err| err.to_string())?);
    }
    Ok(patterns)
}

fn build_options(cli: &Cli) -> Result<SearchOptions, String> {
    let mode = match cli.mode {
        CliMode::Regex => MatchMode::Regex,
        CliMode::Contains => MatchMode::Contains,
        CliMode::Fuzzy => MatchMode::Fuzzy {
            threshold: cli.fuzzy_threshold,
        },
    };

    let patterns = collect_patterns(cli)?;
    if patterns.is_empty() && cli.wheres.is_empty() {
        return Err(
            "at least one --pattern, --builtin, --pattern-file or --where is required".into(),
        );
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

    let mut rules: Vec<ColumnRule> = cli
        .wheres
        .iter()
        .map(|raw| parse_where(raw, &mode, cli.ignore_case))
        .collect::<Result<Vec<_>, _>>()?;
    let global_patterns = if rules.is_empty() {
        patterns
    } else {
        if !patterns.is_empty() {
            // Global patterns become one extra rule so they can be mixed
            // with per-column --where rules under the same --logic.
            rules.push(
                ColumnRule::new(columns.clone(), patterns)
                    .mode(mode.clone())
                    .case_insensitive(cli.ignore_case),
            );
        }
        Vec::new()
    };

    let logic = match cli.logic {
        CliLogic::And => LogicOp::And,
        CliLogic::Or => LogicOp::Or,
    };

    Ok(SearchOptions::new(global_patterns)
        .csv(csv)
        .columns(columns)
        .mode(mode)
        .case_insensitive(cli.ignore_case)
        .rules(rules)
        .logic(logic))
}

fn read_stdin() -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

fn writer_for(path: Option<&Path>) -> io::Result<Box<dyn Write>> {
    match path {
        Some(path) => Ok(Box::new(File::create(path)?)),
        None => Ok(Box::new(io::stdout())),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let options = build_options(&cli).map_err(|e| e.to_string())?;
    let format = ExportFormat::from(cli.format.clone());

    let input = cli.input.clone().unwrap_or_else(|| PathBuf::from("-"));
    let from_stdin = input == Path::new("-");

    let output_path = cli.output.clone();
    let output = |content: &str| write_output(output_path.as_deref(), content);

    // Streaming commands open the output before consuming the input, so an
    // in-place `--output` equal to `--input` would truncate the source.
    let streams_output = matches!(
        cli.command,
        Commands::Filter { .. } | Commands::Replace { .. } | Commands::Merge | Commands::Dedupe
    );
    if streams_output && !from_stdin {
        if let Some(out_path) = &output_path {
            let same_file = out_path == &input
                || (out_path.exists()
                    && std::fs::canonicalize(out_path).ok() == std::fs::canonicalize(&input).ok());
            if same_file {
                return Err(
                    "output path would overwrite the input while streaming; use a different --output"
                        .into(),
                );
            }
        }
    }

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
                let mut count = 0usize;
                for hit in find_matches_iter_from_read(&read_stdin()?[..], options)? {
                    hit?;
                    count += 1;
                }
                count
            } else {
                count_matches(&input, options)?
            };
            output(&format!("{count}\n"))?;
        }
        Commands::Filter { invert } => {
            let mut writer = writer_for(output_path.as_deref())?;
            if from_stdin {
                let data = read_stdin()?;
                let reader = build_reader(&data[..], &options);
                filter_rows_to_writer(reader, &options, !invert, &mut writer)?;
            } else {
                let reader = open_reader_from_path(&input, &options)?;
                filter_rows_to_writer(reader, &options, !invert, &mut writer)?;
            }
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
            let mut writer = writer_for(output_path.as_deref())?;
            if from_stdin {
                let data = read_stdin()?;
                let reader = build_reader(&data[..], &replace.search);
                replace_matches_to_writer(reader, &replace, &mut writer)?;
            } else {
                let reader = open_reader_from_path(&input, &replace.search)?;
                replace_matches_to_writer(reader, &replace, &mut writer)?;
            }
        }
        Commands::Merge => {
            let replace = ReplaceOptions::new(cli.patterns.clone(), "[MERGED]")
                .search(options)
                .only_matched_rows(true)
                .keep_headers(false);
            let mut writer = writer_for(output_path.as_deref())?;
            if from_stdin {
                let data = read_stdin()?;
                let reader = build_reader(&data[..], &replace.search);
                replace_matches_to_writer(reader, &replace, &mut writer)?;
            } else {
                let reader = open_reader_from_path(&input, &replace.search)?;
                replace_matches_to_writer(reader, &replace, &mut writer)?;
            }
        }
        Commands::Report => {
            let report = if from_stdin {
                let data = read_stdin()?;
                let reader = build_reader(&data[..], &options);
                let (hits, rows, fields) = scan_stats_reader(reader, &options)?;
                build_report(&hits, rows, fields)
            } else {
                report_matches(&input, options)?
            };
            output(&export_report(&report, format)?)?;
        }
        Commands::Dedupe => {
            let mut writer = writer_for(output_path.as_deref())?;
            if from_stdin {
                let data = read_stdin()?;
                let reader = build_reader(&data[..], &options);
                dedupe_rows_to_writer(reader, &options, &mut writer)?;
            } else {
                let reader = open_reader_from_path(&input, &options)?;
                dedupe_rows_to_writer(reader, &options, &mut writer)?;
            }
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
