pub struct ContractVersion {
    number: u32,
}

impl ContractVersion {
    pub const CURRENT: Self = Self { number: 1 };

    pub const fn number(&self) -> u32 {
        self.number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_current_contract_is_number_one() {
        assert_eq!(ContractVersion::CURRENT.number(), 1);
    }
}
