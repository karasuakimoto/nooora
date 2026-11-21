// The live trade path enforces these transitions in SQL. This module is the
// checked model those statements follow, and the unit tests pin it.
#![allow(dead_code)]

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub cards: Vec<String>,
    pub gold: i64,
    pub locked: bool,
    pub confirmed: bool,
}

impl Offer {
    pub fn empty() -> Self {
        Self {
            cards: Vec::new(),
            gold: 0,
            locked: false,
            confirmed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub a: Offer,
    pub b: Offer,
}

impl Session {
    pub fn offer_mut(&mut self, side: Side) -> &mut Offer {
        match side {
            Side::A => &mut self.a,
            Side::B => &mut self.b,
        }
    }

    pub fn reset_locks(&mut self) {
        for offer in [&mut self.a, &mut self.b] {
            offer.locked = false;
            offer.confirmed = false;
        }
    }

    /// Any offer edit invalidates both locks and both confirms.
    pub fn edit_offer(&mut self, side: Side, cards: Vec<String>, gold: i64) {
        let offer = self.offer_mut(side);
        offer.cards = cards;
        offer.gold = gold;
        self.reset_locks();
    }

    pub fn lock(&mut self, side: Side) {
        self.offer_mut(side).locked = true;
        self.offer_mut(side).confirmed = false;
    }

    pub fn confirm(&mut self, side: Side) -> Result<(), &'static str> {
        if !self.a.locked || !self.b.locked {
            return Err("both players must lock before confirming");
        }
        self.offer_mut(side).confirmed = true;
        Ok(())
    }

    pub fn ready(&self) -> bool {
        self.a.locked && self.b.locked && self.a.confirmed && self.b.confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session {
            a: Offer::empty(),
            b: Offer::empty(),
        }
    }

    #[test]
    fn editing_an_offer_resets_both_locks() {
        let mut trade = session();
        trade.lock(Side::A);
        trade.lock(Side::B);
        trade.confirm(Side::A).unwrap();
        trade.edit_offer(Side::B, vec!["card1".into()], 25);
        assert!(!trade.a.locked && !trade.b.locked);
        assert!(!trade.a.confirmed && !trade.b.confirmed);
        assert_eq!(trade.b.gold, 25);
    }

    #[test]
    fn confirm_requires_both_locks_and_then_both_confirms() {
        let mut trade = session();
        assert!(trade.confirm(Side::A).is_err());
        trade.lock(Side::A);
        trade.lock(Side::B);
        trade.confirm(Side::A).unwrap();
        assert!(!trade.ready());
        trade.confirm(Side::B).unwrap();
        assert!(trade.ready());
    }
}
