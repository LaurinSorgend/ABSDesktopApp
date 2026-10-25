use super::*;

const HOUR: f64 = 3600.0;

fn item(account: &str, id: &str, title: &str, author: &str, duration: f64) -> Candidate {
    Candidate {
        account_id: account.into(),
        item_id: id.into(),
        server_id: format!("server-of-{account}"),
        title: title.into(),
        title_sort: title.into(),
        author: Some(author.into()),
        duration: Some(duration),
        ..Candidate::default()
    }
}

/// Groups as lists of item ids, only those with more than one member.
fn merged(candidates: &[Candidate], overrides: &[Override]) -> Vec<Vec<String>> {
    group(candidates, overrides)
        .into_iter()
        .filter(|g| g.len() > 1)
        .map(|g| {
            g.iter()
                .map(|(i, _)| candidates[*i].item_id.clone())
                .collect()
        })
        .collect()
}

fn methods(candidates: &[Candidate]) -> Vec<MatchMethod> {
    let mut out = vec![MatchMethod::Single; candidates.len()];
    for g in group(candidates, &[]) {
        for (i, method) in g {
            out[i] = method;
        }
    }
    out
}

#[test]
fn same_item_under_two_users_is_exact() {
    let mut a = item("alice", "li_1", "Dune", "Frank Herbert", 21.0 * HOUR);
    let mut b = item("bob", "li_1", "Dune (edited)", "Someone Else", 1.0 * HOUR);
    a.server_id = "home".into();
    b.server_id = "home".into();
    let c = item("carol", "li_1", "Emma", "Jane Austen", 10.0 * HOUR);
    let candidates = [a, b, c];
    assert_eq!(merged(&candidates, &[]), [["li_1", "li_1"]]);
    assert_eq!(
        methods(&candidates)[..2],
        [MatchMethod::SameItem, MatchMethod::SameItem]
    );
}

#[test]
fn equal_asin_matches_even_with_other_metadata() {
    let mut a = item("a", "1", "Dune", "Frank Herbert", 21.0 * HOUR);
    let mut b = item("b", "2", "Dune: Book One", "Herbert, Frank", 20.0 * HOUR);
    a.asin = Some("B002V1OF70".into());
    b.asin = Some("b002v1of70".into());
    assert_eq!(merged(&[a.clone(), b.clone()], &[]), [["1", "2"]]);
    assert_eq!(methods(&[a, b]), [MatchMethod::Asin, MatchMethod::Asin]);
}

#[test]
fn isbn10_and_isbn13_match() {
    let mut a = item("a", "1", "The Hobbit", "J. R. R. Tolkien", 11.0 * HOUR);
    let mut b = item("b", "2", "Hobbit (Unabridged)", "Tolkien", 2.0 * HOUR);
    a.isbn = Some("0-261-10221-4".into());
    b.isbn = Some("9780261102217".into());
    assert_eq!(methods(&[a, b]), [MatchMethod::Isbn, MatchMethod::Isbn]);
}

#[test]
fn equal_isbn_with_different_asins_stays_apart() {
    let mut a = item("a", "1", "Emma", "Jane Austen", 10.0 * HOUR);
    let mut b = item("b", "2", "Emma", "Jane Austen", 10.0 * HOUR);
    a.isbn = Some("9780261102217".into());
    b.isbn = Some("9780261102217".into());
    a.asin = Some("B000000001".into());
    b.asin = Some("B000000002".into());
    assert!(merged(&[a, b], &[]).is_empty());
}

#[test]
fn title_author_and_duration_match() {
    let mut a = item(
        "a",
        "1",
        "The Hobbit",
        "J.R.R. Tolkien, Christopher Tolkien",
        11.0 * HOUR,
    );
    a.title_sort = "Hobbit, The".into();
    let b = item("b", "2", "Hobbit", "J. R. R. Tolkien", 11.1 * HOUR);
    assert_eq!(
        methods(&[a, b]),
        [MatchMethod::TitleAuthor, MatchMethod::TitleAuthor]
    );
}

#[test]
fn abridged_edition_stays_apart() {
    let full = item("a", "1", "Emma", "Jane Austen", 15.0 * HOUR);
    let abridged = item("b", "2", "Emma", "Jane Austen", 5.5 * HOUR);
    assert!(merged(&[full, abridged], &[]).is_empty());
}

#[test]
fn duration_tolerance_is_two_percent_or_five_minutes() {
    let short = |id, minutes: f64| item("a", id, "Tale", "Ann Author", minutes * 60.0);
    assert_eq!(merged(&[short("1", 60.0), short("2", 64.0)], &[]).len(), 1);
    assert!(merged(&[short("1", 60.0), short("2", 66.0)], &[]).is_empty());
    let long = |id, hours: f64| item("a", id, "Saga", "Ann Author", hours * HOUR);
    assert_eq!(merged(&[long("1", 40.0), long("2", 40.7)], &[]).len(), 1);
    assert!(merged(&[long("1", 40.0), long("2", 41.0)], &[]).is_empty());
}

#[test]
fn different_narrators_stay_apart() {
    let mut a = item("a", "1", "The Hobbit", "J. R. R. Tolkien", 11.0 * HOUR);
    let mut b = item("b", "2", "The Hobbit", "J. R. R. Tolkien", 10.9 * HOUR);
    a.narrator = Some("Andy Serkis".into());
    b.narrator = Some("Rob Inglis".into());
    assert!(merged(&[a.clone(), b.clone()], &[]).is_empty());
    b.narrator = Some("Rob Inglis, Andy Serkis".into());
    assert_eq!(
        merged(&[a, b], &[]).len(),
        1,
        "a shared narrator is no conflict"
    );
}

#[test]
fn abridged_flag_and_ebook_format_keep_works_apart() {
    let mut full = item("a", "1", "Emma", "Jane Austen", 10.0 * HOUR);
    let mut cut = item("b", "2", "Emma", "Jane Austen", 9.9 * HOUR);
    full.abridged = Some(false);
    cut.abridged = Some(true);
    assert!(merged(&[full, cut], &[]).is_empty());
    let mut epub = item("a", "1", "Emma", "Jane Austen", 0.0);
    let mut pdf = item("b", "2", "Emma", "Jane Austen", 0.0);
    let mut other_epub = item("c", "3", "Emma", "Jane Austen", 0.0);
    epub.ebook_format = Some("epub".into());
    pdf.ebook_format = Some("pdf".into());
    other_epub.ebook_format = Some("EPUB".into());
    assert_eq!(merged(&[epub, pdf, other_epub], &[]), [["1", "3"]]);
}

#[test]
fn never_on_title_alone() {
    let mut a = item("a", "1", "Collected Poems", "", 2.0 * HOUR);
    let mut b = item("b", "2", "Collected Poems", "", 2.0 * HOUR);
    a.author = None;
    b.author = Some(" ".into());
    assert!(merged(&[a, b], &[]).is_empty());
}

#[test]
fn ebooks_without_duration_match_each_other_only() {
    let mut a = item("a", "1", "Emma", "Jane Austen", 0.0);
    let mut b = item("b", "2", "Emma", "Jane Austen", 0.0);
    a.duration = None;
    b.duration = Some(0.0);
    let audio = item("c", "3", "Emma", "Jane Austen", 10.0 * HOUR);
    assert_eq!(merged(&[a, b, audio], &[]), [["1", "2"]]);
}

#[test]
fn a_conflict_through_another_member_keeps_works_apart() {
    let mut a = item("a", "1", "Dune", "Frank Herbert", 21.0 * HOUR);
    let mut b = item("b", "2", "Dune", "Frank Herbert", 21.0 * HOUR);
    let mut c = item("c", "3", "Dune", "Frank Herbert", 21.0 * HOUR);
    a.asin = Some("B002V1OF70".into());
    a.narrator = Some("Scott Brick".into());
    b.asin = Some("B002V1OF70".into());
    c.narrator = Some("Simon Vance".into());
    assert_eq!(merged(&[a, b, c], &[]), [["1", "2"]]);
}

#[test]
fn split_and_merge_overrides_win() {
    let mut a = item("a", "1", "Dune", "Frank Herbert", 21.0 * HOUR);
    let mut b = item("b", "2", "Dune", "Frank Herbert", 21.0 * HOUR);
    a.asin = Some("B002V1OF70".into());
    b.asin = Some("B002V1OF70".into());
    let c = item("c", "3", "Something Else", "Other Person", 1.0 * HOUR);
    let candidates = [a, b, c];
    let split = Override::Split {
        account_id: "b".into(),
        item_id: "2".into(),
    };
    assert!(merged(&candidates, std::slice::from_ref(&split)).is_empty());
    let merge = Override::Merge {
        account_id: "c".into(),
        item_id: "3".into(),
        target_account_id: "b".into(),
        target_item_id: "2".into(),
    };
    assert_eq!(merged(&candidates, &[split, merge]), [["2", "3"]]);
    let groups = group(&candidates, &[]);
    assert_eq!(groups.len(), 2);
}
