//! Pasted text in the search field.

use quickswitch::ui::palette::tidy_pasted;

#[test]
fn copy_as_path_quotes_are_removed() {
    assert_eq!(
        tidy_pasted(r#""C:\Users\alice\Projects""#).as_deref(),
        Some(r"C:\Users\alice\Projects")
    );
}

#[test]
fn trailing_newline_space_is_trimmed() {
    // A single-line field turns the copied line's newline into a space.
    assert_eq!(tidy_pasted("quickswitch ").as_deref(), Some("quickswitch"));
    assert_eq!(tidy_pasted(" \"~/docs\" ").as_deref(), Some("~/docs"));
}

#[test]
fn clean_text_is_left_alone() {
    assert_eq!(tidy_pasted("release notes"), None);
    // Only a matching pair of quotes is removed.
    assert_eq!(tidy_pasted("\"half quoted"), None);
    assert_eq!(tidy_pasted("say \"hi\""), None);
}
