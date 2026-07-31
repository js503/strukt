use strukt_editor::{FindError, FindOptions, FindQuery, Revision};

#[test]
fn literal_search_supports_case_whole_word_and_wraparound_navigation() {
    let query = FindQuery::new(
        "cat",
        FindOptions {
            case_sensitive: false,
            whole_word: true,
            regex: false,
        },
    )
    .unwrap();
    let result = query.find_all("Cat scatter cat");
    assert_eq!(result.matches().len(), 2);
    assert_eq!(result.next_after(12), Some(result.matches()[0]));
    assert_eq!(result.previous_before(1), Some(result.matches()[1]));
}

#[test]
fn regex_search_reports_invalid_patterns_and_unicode_ranges() {
    assert!(matches!(
        FindQuery::new("(", FindOptions::regex()),
        Err(FindError::InvalidRegex(_))
    ));
    let result = FindQuery::new("😀.", FindOptions::regex())
        .unwrap()
        .find_all("a😀界 b😀x");
    assert_eq!(result.matches()[0].range.start, 1);
    assert_eq!(result.matches()[0].range.end, 3);
}

#[test]
fn zero_width_regex_search_makes_progress() {
    let result = FindQuery::new(r"\b", FindOptions::regex())
        .unwrap()
        .find_all("one two");
    assert_eq!(result.matches().len(), 4);
}

#[test]
fn replace_all_is_one_revision_bound_transaction() {
    let query = FindQuery::new(r"(\w+)-(\w+)", FindOptions::regex()).unwrap();
    let transaction = query
        .replace_all(Revision::new(7), "a-b c-d", "$2:$1")
        .unwrap();
    assert_eq!(transaction.expected_revision, Revision::new(7));
    assert_eq!(transaction.replacements.len(), 2);
    assert_eq!(transaction.replacements[0].text, "b:a");
    assert_eq!(transaction.replacements[1].text, "d:c");
}
