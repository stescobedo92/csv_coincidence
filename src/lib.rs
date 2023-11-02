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