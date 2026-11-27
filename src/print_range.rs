/// Parse one-based page numbers, preserving requested order without duplicates.
pub fn parse(text: &str, count: usize) -> Result<Vec<usize>, String> {
    if text.trim().is_empty() {
        return Err("Enter pages, for example 1, 3, 5-8.".into());
    }
    let mut pages = Vec::new();
    for part in text.split(',') {
        let bounds: Vec<_> = part.trim().split('-').collect();
        let number = |s: &str| -> Result<usize, String> {
            let s = s.trim();
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err("Use page numbers separated by commas, or ranges such as 2-5.".into());
            }
            let n = s
                .parse::<usize>()
                .map_err(|_| "Page number is too large.".to_string())?;
            if n == 0 || n > count {
                return Err(format!("Page numbers must be between 1 and {count}."));
            }
            Ok(n)
        };
        let (first, last) = match bounds.as_slice() {
            [one] => {
                let n = number(one)?;
                (n, n)
            }
            [first, last] => (number(first)?, number(last)?),
            _ => return Err("Use ranges such as 2-5.".into()),
        };
        if first > last {
            return Err("Range start must be less than or equal to its end.".into());
        }
        for n in first..=last {
            if !pages.contains(&(n - 1)) {
                pages.push(n - 1);
            }
        }
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    #[test]
    fn selected_pages_preserve_order_and_remove_overlaps() {
        assert_eq!(super::parse("3, 1, 2-4, 4", 4).unwrap(), vec![2, 0, 1, 3]);
        assert_eq!(super::parse(" 1 - 3 ", 3).unwrap(), vec![0, 1, 2]);
    }
    #[test]
    fn rejects_invalid_ranges_without_printing_a_partial_selection() {
        for text in [
            "",
            " ",
            "0",
            "4",
            "2-1",
            "1,",
            "1,,2",
            "a",
            "1-2-3",
            "-1",
            "1.5",
            "9999999999999999999999999999999",
        ] {
            assert!(super::parse(text, 3).is_err(), "{text}");
        }
    }
}
