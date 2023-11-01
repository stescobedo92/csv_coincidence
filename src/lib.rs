use csv::Reader;
use regex::Regex;
use std::error::Error;
use std::fs::File;
use std::path::Path;

///The find_partial_matches special function to search partial matches in a CSV file using a customizable regular expression.
/// The function returns a list of all partial matches found.
fn find_partial_matches(file_path: &str, regex_pattern: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let file_extension = Path::new(file_path).extension().and_then(|ext| ext.to_str());
    if file_extension != Some("csv") {
        return Err("The file is not a valid CSV file".into());
    }

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