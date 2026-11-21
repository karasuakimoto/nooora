use poise::BoxFuture;
use poise::serenity_prelude::{Context as SerenityContext, Message};

use crate::error::BotError;
use crate::state::AppState;

const COMMANDS: &[&str] = &[
    "k", "nooora", "d", "drop", "cd", "cooldown", "c", "cards", "quest", "series", "trade", "g",
    "gift", "market", "admin",
];

pub fn strip<'a>(
    ctx: &'a SerenityContext,
    msg: &'a Message,
    data: &'a AppState,
) -> BoxFuture<'a, Result<Option<&'a str>, BotError>> {
    Box::pin(async move {
        let bot_id = data.bot_id();
        let fallback = ctx.cache.current_user().id.get();
        Ok(prefix_slice(msg.content.as_str(), if bot_id == 0 { fallback } else { bot_id }))
    })
}

pub fn prefix_slice(content: &str, bot_id: u64) -> Option<&str> {
    let len = prefix_len(content, bot_id)?;
    Some(&content[..len])
}

pub fn prefix_len(content: &str, bot_id: u64) -> Option<usize> {
    let trimmed = content.trim_start();
    let lead = content.len() - trimmed.len();
    if let Some(after_mention) = strip_mention(trimmed, bot_id) {
        let mention_len = trimmed.len() - after_mention.len();
        let command = after_mention.trim_start();
        let ws = after_mention.len() - command.len();
        let base = lead + mention_len + ws;
        if let Some(f_len) = command_f_len(command) {
            return Some(base + f_len);
        }
        if is_command_token(command) {
            return Some(lead + mention_len);
        }
        return None;
    }
    command_f_len(trimmed).map(|n| lead + n)
}

fn command_f_len(s: &str) -> Option<usize> {
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.eq_ignore_ascii_case(&'f') {
        return None;
    }
    let rest = chars.as_str();
    if is_command_token(rest) {
        Some(first.len_utf8())
    } else {
        None
    }
}

fn is_command_token(s: &str) -> bool {
    let token = s.split_whitespace().next().unwrap_or("");
    COMMANDS.iter().any(|name| name.eq_ignore_ascii_case(token))
}

fn strip_mention<'a>(content: &'a str, bot_id: u64) -> Option<&'a str> {
    let plain = format!("<@{bot_id}>");
    let bang = format!("<@!{bot_id}>");
    content
        .strip_prefix(&plain)
        .or_else(|| content.strip_prefix(&bang))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_short_and_mention_prefixes() {
        assert_eq!(prefix_slice("fk", 42), Some("f"));
        assert_eq!(prefix_slice("  FK", 42), Some("  F"));
        assert_eq!(prefix_slice("fseries Naruto", 42), Some("f"));
        assert_eq!(prefix_slice("ftrade @user", 42), Some("f"));
        assert_eq!(prefix_slice("<@42> fk", 42), Some("<@42> f"));
        assert_eq!(prefix_slice("<@!42> k", 42), Some("<@!42>"));
        assert_eq!(prefix_slice("<@42>k", 42), Some("<@42>"));
        assert!(prefix_slice("for sale", 42).is_none());
        assert!(prefix_slice("hello", 42).is_none());
    }
}
