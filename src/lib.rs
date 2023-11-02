use csv::{Reader, ReaderBuilder, WriterBuilder};
use regex::Regex;
use std::error::Error;
use std::fs::File;
use std::path::Path;

/// Finds partial matches in the CSV file based on the given regular expression pattern.
///
/// # Arguments
///
/// * `file_path` - A string slice representing the file path to the input CSV file.
/// * `regex_pattern` - A string slice representing the regular expression pattern to match against the CSV records.
///
/// # Returns
///
/// A `Result` containing a vector of strings with the partial matches if successful, or an error message if there is any issue during processing.
///
/// # Errors
///
/// Returns an error if the file is not a valid CSV file or if there is an issue with the regular expression pattern.
///
/// # Example
///
/// ```
/// use std::error::Error;
/// use csv_coincidence::find_partial_matches;
///
/// fn main() -> Result<(), Box<dyn Error>> {
///     let file_path = "example.csv";
///     let regex_pattern = r"\bpartial\b"; // Regular expression pattern to match against the CSV records.
///
///     match find_partial_matches(file_path, regex_pattern) {
///         Ok(partial_matches) => {
///             println!("Partial matches found: {:?}", partial_matches);
///             Ok(())
///         }
///         Err(err) => Err(err.into()),
///     }
/// }
/// ```
pub fn find_partial_matches(file_path: &str, regex_pattern: &str) -> Result<Vec<String>, Box<dyn Error>> {
    validate_csv_extension(file_path);

    let file = File::open(file_path)?;
    let mut rdr = Reader::from_reader(file);

    let re = Regex::new(regex_pattern)?;
    let mut partial_matches = Vec::new();

    for result in rdr.records() {
        let record = result?;
        for field in record.iter() {
            if re.is_match(field) {
                partial_matches.push(field.to_string());
            }
        }
    }

    Ok(partial_matches)
}

/// Counts the number of occurrences of a specific pattern in the CSV file.
///
/// # Arguments
///
/// * `file_path` - A string slice representing the file path to the input CSV file.
/// * `pattern` - A string slice representing the regular expression pattern to match against the CSV records.
///
/// # Returns
///
/// A `Result` containing the count of occurrences if successful, or an error message if there is any issue during processing.
///
/// # Errors
///
/// Returns an error if the file is not a valid CSV file or if there is an issue with the regular expression pattern.
///
/// # Example
///
/// ```
/// use std::error::Error;
/// use csv_coincidence::count_coincidences;
///
/// fn main() -> Result<(), Box<dyn Error>> {
///     let file_path = "example.csv";
///     let pattern = r"\bexample\b"; // Regular expression pattern to match against the CSV records.
///
///     match count_coincidences(file_path, pattern) {
///         Ok(count) => {
///             println!("Number of occurrences: {}", count);
///             Ok(())
///         }
///         Err(err) => Err(err.into()),
///     }
/// }
/// ```
pub fn count_coincidences(file_path: &str, patron: &str) -> Result<usize, Box<dyn Error>> {
    validate_csv_extension(file_path);

    let re = Regex::new(patron)?;

    let file = File::open(file_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);

    let mut contador = 0;
    for result in rdr.records() {
        let record = result?;
        for field in record.iter() {
            if re.is_match(field) {
                contador += 1;
            }
        }
    }

    Ok(contador)
}

/// Merges the records in a CSV file that match a specific pattern and replaces those matches with "[MERGED]".
///
/// # Arguments
///
/// * `file_path` - A string slice representing the file path to the input CSV file.
/// * `pattern` - A string slice representing the regular expression pattern to match against the CSV records.
///
/// # Returns
///
/// A `Result` containing a `String` with the merged CSV data if successful, or an error message if there is any issue during processing.
///
/// # Errors
///
/// Returns an error if the file is not a valid CSV file or if there is an issue with the regular expression pattern.
///
/// # Example
///
/// ```
/// use std::error::Error;
/// use csv_coincidence::merge_coincidence;
///
/// fn main() -> Result<(), Box<dyn Error>> {
///     let file_path = "example.csv";
///     let pattern = r"\bmerge\b"; // Regular expression pattern to match against the CSV records.
///
///     match merge_coincidence(file_path, pattern) {
///         Ok(merged_data) => {
///             println!("Merged CSV data:\n{}", merged_data);
///             Ok(())
///         }
///         Err(err) => Err(err.into()),
///     }
/// }
/// ```
pub fn merge_coincidence(file_path: &str, patron: &str) -> Result<String, Box<dyn Error>> {
    validate_csv_extension(file_path);

    let re = Regex::new(patron)?;
    let file = File::open(file_path)?;
    let mut rdr = ReaderBuilder::new().has_headers(true).from_reader(file);
    let mut merged_data: Vec<Vec<String>> = Vec::new();

    for result in rdr.records() {
        let record = result?;
        let mut merged_record: Vec<String> = Vec::new();
        let mut merged = false;

        for field in record.iter() {
            if re.is_match(field) {
                merged_record.push("[MERGED]".to_string());
                merged = true;
            } else {
                merged_record.push(field.to_string());
            }
        }

        if merged {
            merged_data.push(merged_record);
        }
    }

    let mut wtr = WriterBuilder::new().from_writer(vec![]);
    for record in merged_data {
        wtr.write_record(&record)?;
    }

    Ok(String::from_utf8(wtr.into_inner()?)?)
}

/// Validates if the given file path has a ".csv" extension.
///
/// # Arguments
///
/// * `file_path` - A string slice representing the file path to be validated.
///
/// # Returns
///
/// A `Result` containing `Ok(())` if the file has a valid CSV extension, or an error message if the extension is not valid.
///
fn validate_csv_extension(file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let file_extension = Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str());

    match file_extension {
        Some("csv") => Ok(()),
        _ => Err("The file is not a valid CSV file".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_partial_matches() {
        let file_path = "test_data.csv";
        let regex_pattern = r"^[A-Z][a-z]*";

        let matches = find_partial_matches(file_path, regex_pattern).unwrap();
        let mut expected_matches_results = Vec::<String>::new();

        expected_matches_results.push("Jhon".to_string());
        expected_matches_results.push("Marta".to_string());

        assert_eq!(expected_matches_results, matches);
    }

    #[test]
    fn test_find_partial_matches_invalid_file() {
        let file_path = "test_data.txt";
        let regex_pattern = r"^[A-Z][a-z]*";

        let result = find_partial_matches(file_path, regex_pattern);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_partial_matches_no_matches() {
        let file_path = "test_data.csv";
        let regex_pattern = r"^[0-9]+";

        let matches = find_partial_matches(file_path, regex_pattern).unwrap();
        assert_eq!(matches, Vec::<String>::new());
    }

    #[test]
    fn test_find_partial_matches_empty_file() {
        let file_path = "empty.csv";
        let regex_pattern = r"^[A-Z][a-z]*";

        let matches = find_partial_matches(file_path, regex_pattern).unwrap();
        assert_eq!(matches, Vec::<String>::new());
    }

    #[test]
    fn test_find_partial_matches_empty_regex() {
        let file_path = "test_data.csv";
        let regex_pattern = r"";  // Empty regular expression

        let result = find_partial_matches(file_path, regex_pattern);
        assert!(result.is_err());
    }
}