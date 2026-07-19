# csv_coincidence

Search, count, filter, replace, and report pattern matches inside CSV files.

`csv_coincidence` is a Rust library and CLI focused on finding partial matches in CSV data using regular expressions, substring search, or fuzzy similarity.

## Features

- Find matches with **row / column context**
- Count occurrences across one or more patterns
- Filter rows that match (or invert the filter)
- Replace matches with custom text or regex capture groups (`$1`, `$2`, …)
- Merge helper that replaces matches with `[MERGED]`
- Column targeting by **name** or **index**
- CSV dialect options: delimiter, headers, quote, trim
- Fuzzy matching (normalized Levenshtein)
- Deduplicate rows by matched-column fingerprints
- Summary **report** (totals, columns touched, top values)
- Export hits/reports as **text**, **CSV**, **TSV**, or **JSON**
- Read from **file path**, **memory**, or **stdin**
- Typed errors via `CsvCoincidenceError`

## Install

### Library

```toml
[dependencies]
csv_coincidence = { version = "0.2", default-features = false }
```

### CLI

```bash
cargo install csv_coincidence
```

Or from this repository:

```bash
cargo install --path . --features cli
```

## CLI usage

```bash
# Find matches with context (JSON)
csv-coincidence find -i data.csv -p '^[A-Z][a-z]+' -f json

# Count matches in one column
csv-coincidence count -i data.csv -p '@example\.com' -c email

# Keep matching rows
csv-coincidence filter -i data.csv -p 'Madrid' -c city

# Replace / redact
csv-coincidence replace -i data.csv -p '@.*' -c email -r '[REDACTED]'

# Compatibility merge
csv-coincidence merge -i data.csv -p '^Jhon$'

# Report
csv-coincidence report -i data.csv -p 'example\.com' -p 'test\.org' -f text

# Fuzzy match
csv-coincidence find -i data.csv -p John --mode fuzzy --fuzzy-threshold 0.5 -c name

# Stdin / stdout pipelines
cat data.csv | csv-coincidence filter -i - -p 'Berlin' > berlin.csv
```

## Library usage

```rust
use csv_coincidence::{find_matches, SearchOptions, ColumnSelector};

fn main() -> csv_coincidence::Result<()> {
    let options = SearchOptions::new([r"@example\.com"])
        .columns(ColumnSelector::Names(vec!["email".into()]));

    for hit in find_matches("data.csv", options)? {
        println!(
            "row={} column={} value={}",
            hit.row,
            hit.column_name.as_deref().unwrap_or("?"),
            hit.value
        );
    }
    Ok(())
}
```

### Compatibility helpers

The original helpers remain available:

```rust
use csv_coincidence::{count_coincidences, find_partial_matches, merge_coincidence};

let matches = find_partial_matches("data.csv", r"^[A-Z][a-z]*")?;
let count = count_coincidences("data.csv", r"example")?;
let merged = merge_coincidence("data.csv", r"^Jhon$")?;
```

## License

MIT OR Apache-2.0
