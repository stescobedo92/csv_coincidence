# csv_coincidence

Often in the realm of data processing, CSV files are used to store tabular data, and it is important to be able to efficiently search and analyze that data this is the motivation behind the `csv_coincidence` that is a library focused on the searches for partial matches in a CSV file using a customizable regular expression. This function is used to process CSV files and search for partial matches within the text strings found in the file.

# Usage

```rust
use csv_coincidence::find_partial_matches;

fn main() -> Result<(), Box<dyn Error>> {
    let file_path = "example.csv";  // Replace with the path of your CSV file
    let regex_pattern = r"^[A-Z][a-z]*";  // Replace with the regular expression

    match find_partial_matches(file_path, regex_pattern) {
        Ok(matches) => {
            println!("Partial Matches:");
            for match_str in matches {
                println!("{}", match_str);
            }
        }
        Err(err) => {
            eprintln!("Error: {}", err);
        }
    }

    Ok(())
}
```
# License
This project is licensed under the MIT license.
