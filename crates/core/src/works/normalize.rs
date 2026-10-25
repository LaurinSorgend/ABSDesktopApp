//! Normalised forms of titles, names and identifiers, for matching items across servers.

use unicode_normalization::UnicodeNormalization;

/// Leading articles stripped when the server sent no `titleIgnorePrefix`.
const ARTICLES: [&str; 3] = ["the", "a", "an"];

/// NFKC, lowercase, punctuation as spaces, runs of spaces collapsed.
pub fn text(value: &str) -> String {
    let folded: String = value
        .nfkc()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The title without its leading article. `title_sort` is the server's `titleIgnorePrefix`
/// form ("Hobbit, The"); when it moved a prefix to the end, that prefix is dropped.
pub fn title(title: &str, title_sort: &str) -> String {
    if let Some(base) = without_moved_prefix(title, title_sort) {
        return text(base);
    }
    let normal = text(title);
    for article in ARTICLES {
        if let Some(rest) = normal
            .strip_prefix(article)
            .and_then(|r| r.strip_prefix(' '))
        {
            return rest.to_owned();
        }
    }
    normal
}

fn without_moved_prefix<'t>(title: &str, title_sort: &'t str) -> Option<&'t str> {
    let (base, prefix) = title_sort.rsplit_once(", ")?;
    let starts = title
        .get(..prefix.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(prefix));
    (title != title_sort && starts && !base.trim().is_empty()).then_some(base)
}

/// The first of comma, ampersand or semicolon separated names, normalised.
pub fn primary_author(author: &str) -> Option<String> {
    names(author).into_iter().next()
}

/// Each name of a comma, ampersand or semicolon separated list, normalised.
pub fn names(list: &str) -> Vec<String> {
    list.split([',', '&', ';'])
        .map(text)
        .filter(|name| !name.is_empty())
        .collect()
}

/// An ASIN in upper case, or `None` when it is not ten letters and digits.
pub fn asin(value: &str) -> Option<String> {
    let asin: String = value
        .trim()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    (asin.len() == 10 && asin.chars().all(|c| c.is_ascii_alphanumeric())).then_some(asin)
}

/// ISBN-13 digits; an ISBN-10 is converted. `None` when the check digit is wrong.
pub fn isbn13(value: &str) -> Option<String> {
    let chars: String = value
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    match chars.len() {
        13 if valid_isbn13(&chars) => Some(chars),
        10 if valid_isbn10(&chars) => {
            let body = format!("978{}", &chars[..9]);
            let check = isbn13_check(&body)?;
            Some(format!("{body}{check}"))
        }
        _ => None,
    }
}

fn valid_isbn10(isbn: &str) -> bool {
    let mut sum = 0;
    for (i, c) in isbn.chars().enumerate() {
        let digit = match c {
            'X' if i == 9 => 10,
            c => match c.to_digit(10) {
                Some(d) => d,
                None => return false,
            },
        };
        sum += digit * (10 - i as u32);
    }
    sum % 11 == 0
}

fn valid_isbn13(isbn: &str) -> bool {
    isbn13_check(&isbn[..12]).is_some_and(|check| isbn.ends_with(check))
}

fn isbn13_check(first12: &str) -> Option<char> {
    let mut sum = 0;
    for (i, c) in first12.chars().enumerate() {
        let digit = c.to_digit(10)?;
        sum += if i % 2 == 0 { digit } else { digit * 3 };
    }
    char::from_digit((10 - sum % 10) % 10, 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_folds_case_width_and_punctuation() {
        assert_eq!(
            text("  Harry Potter: The Philosopher’s Stone! "),
            "harry potter the philosopher s stone"
        );
        assert_eq!(text("ＡＢＣ"), "abc");
        assert_eq!(text("J.R.R. Tolkien"), "j r r tolkien");
        assert_eq!(text("Café"), text("Cafe\u{301}"));
    }

    #[test]
    fn title_drops_leading_article() {
        assert_eq!(title("The Hobbit", "Hobbit, The"), "hobbit");
        assert_eq!(title("The Hobbit", "The Hobbit"), "hobbit");
        assert_eq!(
            title("A Study in Scarlet", "A Study in Scarlet"),
            "study in scarlet"
        );
        assert_eq!(title("Der Process", "Process, Der"), "process");
        assert_eq!(title("Anathem", "Anathem"), "anathem");
        assert_eq!(title("Yes, Minister", "Yes, Minister"), "yes minister");
    }

    #[test]
    fn primary_author_is_the_first_name() {
        assert_eq!(
            primary_author("Terry Pratchett, Neil Gaiman").as_deref(),
            Some("terry pratchett")
        );
        assert_eq!(
            primary_author("Douglas Preston & Lincoln Child").as_deref(),
            Some("douglas preston")
        );
        assert_eq!(primary_author(" , "), None);
        assert_eq!(
            names("Rob Inglis; Andy Serkis"),
            ["rob inglis", "andy serkis"]
        );
    }

    #[test]
    fn isbn10_becomes_isbn13() {
        assert_eq!(isbn13("0-261-10221-4").as_deref(), Some("9780261102217"));
        assert_eq!(
            isbn13("978-0-261-10221-7").as_deref(),
            Some("9780261102217")
        );
        assert_eq!(isbn13("080442957X").as_deref(), Some("9780804429573"));
        assert_eq!(isbn13("0-261-10221-5"), None);
        assert_eq!(isbn13("978-0-261-10221-8"), None);
        assert_eq!(isbn13("12345"), None);
    }

    #[test]
    fn asin_is_upper_case_and_ten_long() {
        assert_eq!(asin(" b002v0qk4c ").as_deref(), Some("B002V0QK4C"));
        assert_eq!(asin("B002V0QK4"), None);
        assert_eq!(asin("B002V0QK4-"), None);
    }
}
