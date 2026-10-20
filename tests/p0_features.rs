use csv_coincidence::{
    build_reader, builtin_pattern, export_hits, filter_rows_reader, filter_rows_to_writer,
    find_matches_from_read, find_matches_iter_from_read, is_valid_credit_card, is_valid_dni,
    is_valid_iban, is_valid_nie, parse_patterns, read_pattern_file, replace_matches_reader,
    replace_matches_to_writer, ColumnRule, ColumnSelector, ExportFormat, LogicOp, MatchMode,
    ReplaceOptions, SearchOptions,
};
use std::io::Write;

const DATA: &[u8] = b"name,city,email\n\
    Jhon,Madrid,jhon@example.com\n\
    Marta,Berlin,marta@test.org\n\
    anna,Paris,anna@example.com\n";

fn names(columns: &[&str]) -> ColumnSelector {
    ColumnSelector::Names(columns.iter().map(|c| c.to_string()).collect())
}

#[test]
fn streaming_iter_matches_find_and_yields_incrementally() {
    let options = SearchOptions::new([r"example\.com"]).columns(names(&["email"]));

    let collected = find_matches_from_read(DATA, options.clone()).unwrap();
    let mut iter = find_matches_iter_from_read(DATA, options).unwrap();

    let first = iter.next().unwrap().unwrap();
    assert_eq!(first.row, 1);
    assert_eq!(first.value, "jhon@example.com");

    let rest: Vec<_> = iter.collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(rest.len() + 1, collected.len());
    assert_eq!(collected.len(), 2);
}

#[test]
fn regex_hits_carry_span_and_captures() {
    let data: &[u8] = b"id,contact\n1,write to jhon@example.com please\n";
    let options = SearchOptions::new([r"(\w+)@(example\.com)"]).columns(names(&["contact"]));
    let hits = find_matches_from_read(data, options).unwrap();

    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.matched_text, "jhon@example.com");
    assert_eq!(&hit.value[hit.start..hit.end], "jhon@example.com");
    assert_eq!(
        hit.captures,
        vec![Some("jhon".to_string()), Some("example.com".to_string())]
    );

    let json = export_hits(&hits, ExportFormat::Json).unwrap();
    assert!(json.contains("\"matched_text\""));
    assert!(json.contains("\"captures\""));
}

#[test]
fn contains_span_preserves_original_case_and_fuzzy_spans_whole_field() {
    let options = SearchOptions::new(["EXAMPLE"])
        .mode(MatchMode::Contains)
        .case_insensitive(true)
        .columns(names(&["email"]));
    let hits = find_matches_from_read(DATA, options).unwrap();
    assert_eq!(hits[0].matched_text, "example");
    assert_eq!(&hits[0].value[hits[0].start..hits[0].end], "example");

    let fuzzy = SearchOptions::new(["John"])
        .mode(MatchMode::Fuzzy { threshold: 0.5 })
        .columns(names(&["name"]));
    let hits = find_matches_from_read(DATA, fuzzy).unwrap();
    let jhon = hits.iter().find(|h| h.value == "Jhon").unwrap();
    assert_eq!(jhon.start, 0);
    assert_eq!(jhon.end, jhon.value.len());
    assert!(jhon.score.is_some());
}

#[test]
fn invalid_fuzzy_threshold_is_rejected() {
    let options = SearchOptions::new(["John"]).mode(MatchMode::Fuzzy { threshold: 1.5 });
    assert!(find_matches_from_read(DATA, options).is_err());
}

#[test]
fn column_rules_and_or_not_filter_rows() {
    let in_madrid = ColumnRule::new(names(&["city"]), ["Madrid"]);
    let example_mail = ColumnRule::new(names(&["email"]), [r"example\.com"]);

    // AND: only the Jhon row is in Madrid with an example.com address.
    let and_options =
        SearchOptions::with_rules([in_madrid.clone(), example_mail.clone()], LogicOp::And);
    let reader = build_reader(DATA, &and_options);
    let filtered = filter_rows_reader(reader, &and_options, true).unwrap();
    assert_eq!(filtered.lines().count(), 2); // header + Jhon
    assert!(filtered.contains("Jhon"));

    // OR: Madrid rows plus every example.com row (Jhon and anna).
    let or_options = SearchOptions::with_rules([in_madrid, example_mail.clone()], LogicOp::Or);
    let reader = build_reader(DATA, &or_options);
    let filtered = filter_rows_reader(reader, &or_options, true).unwrap();
    assert_eq!(filtered.lines().count(), 3); // header + Jhon + anna

    // NOT: example.com rows whose name is not Jhon -> only anna.
    let not_jhon = ColumnRule::new(names(&["name"]), ["Jhon"]).negated(true);
    let not_options = SearchOptions::with_rules([example_mail, not_jhon], LogicOp::And);
    let reader = build_reader(DATA, &not_options);
    let filtered = filter_rows_reader(reader, &not_options, true).unwrap();
    assert_eq!(filtered.lines().count(), 2); // header + anna
    assert!(filtered.contains("anna"));
    assert!(!filtered.contains("Jhon"));
}

#[test]
fn column_rules_and_scopes_find_hits_to_satisfying_rows() {
    let options = SearchOptions::with_rules(
        [
            ColumnRule::new(names(&["city"]), ["Madrid"]),
            ColumnRule::new(names(&["email"]), [r"example\.com"]),
        ],
        LogicOp::And,
    );
    let hits = find_matches_from_read(DATA, options).unwrap();
    // Both rule hits (city + email) from the single satisfying row.
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.row == 1));
}

#[test]
fn writer_variants_stream_output_and_count_rows() {
    let options = SearchOptions::new([r"example\.com"]).columns(names(&["email"]));

    let mut buffer = Vec::new();
    let written =
        filter_rows_to_writer(build_reader(DATA, &options), &options, true, &mut buffer).unwrap();
    assert_eq!(written, 2);
    let streamed = String::from_utf8(buffer).unwrap();
    let expected = filter_rows_reader(build_reader(DATA, &options), &options, true).unwrap();
    assert_eq!(streamed, expected);

    let replace = ReplaceOptions::new([r"@.*"], "[REDACTED]")
        .search(options)
        .only_matched_rows(false);
    let mut buffer = Vec::new();
    let written =
        replace_matches_to_writer(build_reader(DATA, &replace.search), &replace, &mut buffer)
            .unwrap();
    assert_eq!(written, 3);
    let streamed = String::from_utf8(buffer).unwrap();
    let expected = replace_matches_reader(build_reader(DATA, &replace.search), &replace).unwrap();
    assert_eq!(streamed, expected);
    assert!(streamed.contains("[REDACTED]"));
}

#[test]
fn builtin_patterns_and_validators() {
    let email = builtin_pattern("email").unwrap();
    let hits = find_matches_from_read(DATA, SearchOptions::new([email])).unwrap();
    assert_eq!(hits.len(), 3); // one email address per row

    assert!(builtin_pattern("credit_card").is_some()); // `_` alias for `-`
    assert!(builtin_pattern("nope").is_none());

    assert!(is_valid_credit_card("4242 4242 4242 4242"));
    assert!(!is_valid_credit_card("4242 4242 4242 4241"));
    assert!(!is_valid_credit_card("1234"));

    assert!(is_valid_iban("ES91 2100 0418 4502 0005 1332"));
    assert!(!is_valid_iban("ES91 2100 0418 4502 0005 1333"));
    assert!(!is_valid_iban("not-an-iban"));

    assert!(is_valid_dni("12345678Z"));
    assert!(!is_valid_dni("12345678A"));
    assert!(is_valid_nie("X2482300W"));
    assert!(!is_valid_nie("X2482300A"));

    // Malformed UTF-8-shaped input must be rejected, never panic.
    assert!(!is_valid_dni("123456áZ"));
    assert!(!is_valid_nie("X12345áZ"));
    assert!(!is_valid_dni(""));
}

#[test]
fn pattern_file_parsing_ignores_comments_and_blanks() {
    let parsed = parse_patterns("# comment\n\nMadrid\n  Berlin  \n#Paris\n");
    assert_eq!(parsed, vec!["Madrid".to_string(), "Berlin".to_string()]);

    let mut file = tempfile::NamedTempFile::new().unwrap();
    writeln!(file, "example\\.com\n# ignored\ntest\\.org").unwrap();
    let loaded = read_pattern_file(file.path()).unwrap();
    assert_eq!(loaded.len(), 2);

    let hits = find_matches_from_read(DATA, SearchOptions::new(loaded)).unwrap();
    assert_eq!(hits.len(), 3); // every email matches one of the two domains
}
