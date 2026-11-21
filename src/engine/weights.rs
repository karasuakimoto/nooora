// Drop rolls apply this formula in SQL. The tests pin the stacking cap.
#![allow(dead_code)]

#[derive(Debug, Clone, Copy)]
pub struct Boosts<'a> {
    pub guild_series: Option<&'a str>,
    pub guild_multiplier: f64,
    pub event_series: Option<&'a str>,
    pub event_multiplier: f64,
    pub cap: f64,
}

pub fn effective_weight(base: i32, series_id: &str, boosts: Boosts<'_>) -> f64 {
    let base = base.max(1) as f64;
    let mut factor = 1.0;
    if boosts.guild_series.is_some_and(|id| id == series_id) {
        factor *= boosts.guild_multiplier.max(1.0);
    }
    if boosts.event_series.is_some_and(|id| id == series_id) {
        factor *= boosts.event_multiplier.max(1.0);
    }
    base * factor.min(boosts.cap.max(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_and_event_boosts_stack_under_the_cap() {
        let boosts = Boosts {
            guild_series: Some("086c62"),
            guild_multiplier: 3.0,
            event_series: Some("086c62"),
            event_multiplier: 2.0,
            cap: 9.0,
        };
        assert_eq!(effective_weight(100, "086c62", boosts), 600.0);
        assert_eq!(effective_weight(100, "ffffff", boosts), 100.0);
    }

    #[test]
    fn cap_limits_stacked_multipliers() {
        let boosts = Boosts {
            guild_series: Some("086c62"),
            guild_multiplier: 5.0,
            event_series: Some("086c62"),
            event_multiplier: 5.0,
            cap: 9.0,
        };
        assert_eq!(effective_weight(10, "086c62", boosts), 90.0);
    }
}
