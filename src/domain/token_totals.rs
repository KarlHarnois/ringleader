use super::money::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TotalsWindow {
    Today,
    LastSevenDays,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenTotals {
    window: TotalsWindow,
    tokens: u64,
    list_price: Money,
}

impl TokenTotals {
    pub fn new(window: TotalsWindow, tokens: u64, list_price: Money) -> Self {
        Self {
            window,
            tokens,
            list_price,
        }
    }

    pub fn window(&self) -> TotalsWindow {
        self.window
    }

    pub fn tokens(&self) -> u64 {
        self.tokens
    }

    pub fn list_price(&self) -> &Money {
        &self.list_price
    }
}
