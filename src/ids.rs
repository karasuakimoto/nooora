use crate::error::BotError;

const ALPHABET: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
    'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'M', 'N',
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '2', '3', '4', '5', '6', '7', '8', '9',
];

pub fn card_uid() -> String {
    short_id(8)
}

pub fn trade_id() -> String {
    short_id(12)
}

pub fn short_id(len: usize) -> String {
    // nanoid's custom alphabet keeps card codes short and URL-safe.
    nanoid::nanoid!(len, ALPHABET)
}

pub fn snowflake_i64(id: u64) -> Result<i64, BotError> {
    i64::try_from(id).map_err(|_| BotError::user_message("that Discord id does not fit in a signed 64-bit key"))
}

pub fn parse_card_list(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_schema_shape() {
        let card = card_uid();
        let trade = trade_id();
        assert_eq!(card.len(), 8);
        assert_eq!(trade.len(), 12);
        assert!(card.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(trade.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn card_lists_split_on_commas_and_spaces() {
        assert_eq!(
            parse_card_list("ab12cd34, ZX98QW12  qq11ww22"),
            vec!["ab12cd34", "ZX98QW12", "qq11ww22"]
        );
    }
}
