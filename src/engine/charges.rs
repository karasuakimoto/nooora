use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pool {
    pub charges: i32,
    pub next_regen_at: Option<DateTime<Utc>>,
}

pub fn reconcile(mut pool: Pool, now: DateTime<Utc>, max: i32, every: Duration) -> Pool {
    let step = chrono_step(every);
    if pool.charges >= max {
        pool.charges = max;
        pool.next_regen_at = None;
        return pool;
    }
    let Some(mut next) = pool.next_regen_at else {
        pool.next_regen_at = Some(now + step);
        return pool;
    };
    while pool.charges < max && now >= next {
        pool.charges += 1;
        if pool.charges >= max {
            pool.charges = max;
            pool.next_regen_at = None;
            return pool;
        }
        next += step;
    }
    pool.next_regen_at = Some(next);
    pool
}

pub fn try_spend(
    pool: Pool,
    now: DateTime<Utc>,
    max: i32,
    every: Duration,
) -> Result<Pool, DateTime<Utc>> {
    let pool = reconcile(pool, now, max, every);
    if pool.charges <= 0 {
        return Err(pool
            .next_regen_at
            .unwrap_or_else(|| now + chrono_step(every)));
    }
    let mut spent = pool;
    spent.charges -= 1;
    if spent.charges >= max {
        spent.charges = max;
        spent.next_regen_at = None;
    } else if spent.next_regen_at.is_none() {
        spent.next_regen_at = Some(now + chrono_step(every));
    }
    Ok(spent)
}

fn chrono_step(every: Duration) -> ChronoDuration {
    ChronoDuration::seconds(every.as_secs().max(1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + secs, 0).unwrap()
    }

    #[test]
    fn regenerates_from_the_stored_timestamp() {
        let start = Pool {
            charges: 1,
            next_regen_at: Some(at(0)),
        };
        let later = reconcile(start, at(120), 3, Duration::from_secs(60));
        assert_eq!(later.charges, 3);
        assert!(later.next_regen_at.is_none());
    }

    #[test]
    fn spend_schedules_the_next_charge_only_when_below_max() {
        let full = Pool {
            charges: 3,
            next_regen_at: None,
        };
        let left = try_spend(full, at(0), 3, Duration::from_secs(60)).unwrap();
        assert_eq!(left.charges, 2);
        assert_eq!(left.next_regen_at, Some(at(60)));

        let empty = try_spend(
            Pool {
                charges: 0,
                next_regen_at: Some(at(60)),
            },
            at(0),
            3,
            Duration::from_secs(60),
        );
        assert!(empty.is_err());
    }

    #[test]
    fn partial_regen_keeps_the_remaining_wait() {
        let pool = Pool {
            charges: 0,
            next_regen_at: Some(at(100)),
        };
        let mid = reconcile(pool, at(100), 3, Duration::from_secs(100));
        assert_eq!(mid.charges, 1);
        assert_eq!(mid.next_regen_at, Some(at(200)));
    }
}
