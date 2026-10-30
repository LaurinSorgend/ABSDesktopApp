//! What folder names say about a book. Follows `getBookDataFromDir` and `parseNameString` of ABS.

use std::sync::LazyLock;

use regex::Regex;

static ASIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?: |^)\[([A-Z0-9]{10})\]( |$)").expect("valid regex"));
static NARRATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*) \{(.*)\}$").expect("valid regex"));
static SEQUENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?P<label>vol\.? |volume |book )?(?P<seq>\d{0,3}(?:\.\d{1,2})?)(?P<dot>\.?)(?: (?P<rest>.*))?$",
    )
    .expect("valid regex")
});
static YEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ *\(?([0-9]{4})\)? * - *(.+)").expect("valid regex"));
static CJK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\x{4e00}-\x{9fff}\x{3040}-\x{30ff}\x{31f0}-\x{31ff}]").expect("valid regex")
});

const SORT_PREFIXES: [&str; 2] = ["the ", "a "];

/// Metadata from a book's path relative to the chosen folder.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderData {
    pub title: String,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub series: Option<String>,
    pub sequence: Option<String>,
    pub published_year: Option<String>,
    pub asin: Option<String>,
}

/// Reads `Author/Series/Title`, with ASIN, narrator, sequence and year in the title folder.
pub fn folder_data(rel_path: &str) -> FolderData {
    let mut parts: Vec<&str> = rel_path.split('/').collect();
    let title_folder = parts.pop().unwrap_or_default();
    let series = (parts.len() > 1).then(|| parts.pop()).flatten();
    let author = parts.pop();

    let (rest, asin) = take_asin(title_folder);
    let (rest, narrators) = take_narrators(&rest);
    let (rest, sequence) = match series {
        Some(_) => take_sequence(&rest),
        None => (rest, None),
    };
    let (title, published_year) = take_year(&rest);
    FolderData {
        title,
        authors: author.map(parse_names).unwrap_or_default(),
        narrators,
        series: series.map(str::to_owned),
        sequence,
        published_year,
        asin,
    }
}

fn take_asin(folder: &str) -> (String, Option<String>) {
    let Some(found) = ASIN.captures(folder) else {
        return (folder.to_owned(), None);
    };
    let (start, end) = (
        found.get(0).expect("whole match").start(),
        found.get(2).expect("group").start(),
    );
    let rest = format!("{}{}", &folder[..start], &folder[end..]);
    (rest.trim().to_owned(), Some(found[1].to_owned()))
}

fn take_narrators(folder: &str) -> (String, Vec<String>) {
    match NARRATOR.captures(folder) {
        Some(found) => (found[1].to_owned(), parse_names(&found[2])),
        None => (folder.to_owned(), Vec::new()),
    }
}

/// Takes `Book 2`, `Vol. 3`, `6.` or `0.5` out of one of the ` - ` parts. A bare number
/// followed by words stays, so "101 Dalmatians" keeps its title.
fn take_sequence(folder: &str) -> (String, Option<String>) {
    let mut parts: Vec<String> = folder.split(" - ").map(str::to_owned).collect();
    for index in 0..parts.len() {
        let Some(found) = SEQUENCE.captures(&parts[index]) else {
            continue;
        };
        let rest = found.name("rest").map(|m| m.as_str().to_owned());
        let labelled = found.name("label").is_some() || !found["dot"].is_empty();
        if rest.is_some() && !labelled {
            continue;
        }
        let sequence = number_text(&found["seq"]);
        match rest.filter(|r| !r.is_empty()) {
            Some(rest) => parts[index] = rest,
            None => {
                parts.remove(index);
            }
        }
        return (parts.join(" - "), Some(sequence));
    }
    (folder.to_owned(), None)
}

/// `02` becomes `2`; `0.5` stays.
fn number_text(text: &str) -> String {
    text.parse::<f64>()
        .map_or_else(|_| text.to_owned(), |n| n.to_string())
}

fn take_year(folder: &str) -> (String, Option<String>) {
    match YEAR.captures(folder) {
        Some(found) => (found[2].to_owned(), Some(found[1].to_owned())),
        None => (folder.to_owned(), None),
    }
}

/// Names from `Jon Smith, Jane Smith`, `Smith, Jon, Smith, Jane`, `A & B`, `A and B` or `A; B`.
pub fn parse_names(text: &str) -> Vec<String> {
    let separator = ["&", " and ", ";"].into_iter().find(|s| text.contains(s));
    let chunks: Vec<&str> = match separator {
        Some(separator) => text.split(separator).flat_map(|c| c.split(',')).collect(),
        None => text.split(',').collect(),
    };
    let chunks: Vec<&str> = chunks.into_iter().map(str::trim).collect();
    if chunks.first().is_some_and(|c| CJK.is_match(c)) {
        return chunks.into_iter().map(str::to_owned).collect();
    }
    let last_first = text.contains(',') && chunks.len() > 1 && !chunks[0].contains(' ');
    let names: Vec<String> = if last_first {
        chunks
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[last, first]| format!("{first} {last}"))
            .collect()
    } else {
        chunks.into_iter().map(str::to_owned).collect()
    };
    let mut unique: Vec<String> = Vec::new();
    for name in names {
        if !name.trim().is_empty() && !unique.contains(&name) {
            unique.push(name);
        }
    }
    unique
}

/// The title without a leading article, for sorting.
pub fn title_ignore_prefix(title: &str) -> String {
    let lower = title.to_lowercase();
    SORT_PREFIXES
        .iter()
        .find(|prefix| lower.starts_with(**prefix))
        .and_then(|prefix| title.get(prefix.len()..))
        .map_or_else(|| title.to_owned(), |rest| rest.trim_start().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_author_series_and_title() {
        let data = folder_data(
            "Terry Goodkind/Sword of Truth/Vol 1 - 1994 - Wizards First Rule {Sam Tsoutsouvas}",
        );
        assert_eq!(data.authors, ["Terry Goodkind"]);
        assert_eq!(data.series.as_deref(), Some("Sword of Truth"));
        assert_eq!(data.narrators, ["Sam Tsoutsouvas"]);
        assert_eq!(data.published_year.as_deref(), Some("1994"));
        assert_eq!(data.title, "Wizards First Rule");
    }

    #[test]
    fn two_folders_are_author_and_title() {
        let data = folder_data("Oscar Wilde/The Happy Prince");
        assert_eq!(data.authors, ["Oscar Wilde"]);
        assert_eq!(data.series, None);
        assert_eq!(data.title, "The Happy Prince");
    }

    #[test]
    fn deeper_folders_are_ignored() {
        let data = folder_data("Audio/English/Author/Series/Title");
        assert_eq!(data.authors, ["Author"]);
        assert_eq!(data.series.as_deref(), Some("Series"));
    }

    #[test]
    fn reads_the_asin() {
        let data = folder_data("Dune [B0015T963C]");
        assert_eq!(data.title, "Dune");
        assert_eq!(data.asin.as_deref(), Some("B0015T963C"));
        assert_eq!(folder_data("Dune [short]").asin, None);
    }

    fn sequence_of(title: &str) -> (String, Option<String>) {
        let data = folder_data(&format!("Author/Series/{title}"));
        (data.title, data.sequence)
    }

    #[test]
    fn reads_the_sequence_like_abs() {
        for (folder, title, sequence) in [
            ("Book 2 - Title - Subtitle", "Title - Subtitle", "2"),
            ("Title - Subtitle - Vol 12", "Title - Subtitle", "12"),
            ("Title - volume 9 - Subtitle", "Title - Subtitle", "9"),
            ("Vol. 3 Title Here - Subtitle", "Title Here - Subtitle", "3"),
            ("Volume 12. Title - Subtitle", "Title - Subtitle", "12"),
            ("100 - Book Title", "Book Title", "100"),
            ("6. Title", "Title", "6"),
            ("0.5 - Book Title", "Book Title", "0.5"),
        ] {
            assert_eq!(
                sequence_of(folder),
                (title.to_owned(), Some(sequence.to_owned())),
                "{folder}"
            );
        }
    }

    #[test]
    fn a_number_in_the_title_is_no_sequence() {
        assert_eq!(
            sequence_of("101 Dalmatians"),
            ("101 Dalmatians".to_owned(), None)
        );
        let (title, sequence) = sequence_of("101. Dalmatians");
        assert_eq!(
            (title.as_str(), sequence.as_deref()),
            ("Dalmatians", Some("101"))
        );
    }

    #[test]
    fn without_a_series_a_number_stays_in_the_title() {
        assert_eq!(folder_data("Author/6. Title").title, "6. Title");
    }

    #[test]
    fn reads_the_year_before_the_title() {
        let data = folder_data("(1965) - Dune");
        assert_eq!(data.title, "Dune");
        assert_eq!(data.published_year.as_deref(), Some("1965"));
    }

    #[test]
    fn parses_name_lists() {
        assert_eq!(
            parse_names("Jon Smith, Jane Doe"),
            ["Jon Smith", "Jane Doe"]
        );
        assert_eq!(parse_names("Smith, Jon"), ["Jon Smith"]);
        assert_eq!(
            parse_names("Friedman, Milton & Friedman, Rose"),
            ["Milton Friedman", "Rose Friedman"]
        );
        assert_eq!(
            parse_names("Jon Smith and Jane Doe"),
            ["Jon Smith", "Jane Doe"]
        );
        assert_eq!(
            parse_names("Jon Smith; Jane Doe; Jon Smith"),
            ["Jon Smith", "Jane Doe"]
        );
        assert_eq!(parse_names("村上春樹, 太宰治"), ["村上春樹", "太宰治"]);
        assert!(parse_names(" ").is_empty());
    }

    #[test]
    fn sort_title_drops_the_article() {
        assert_eq!(title_ignore_prefix("The Good Book"), "Good Book");
        assert_eq!(title_ignore_prefix("A Tale"), "Tale");
        assert_eq!(title_ignore_prefix("Theory"), "Theory");
    }
}
