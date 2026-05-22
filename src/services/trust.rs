#[cfg(test)]
pub fn initial_score(verified_identity: bool, successful_cases: u32) -> u8 {
    let base = if verified_identity { 60 } else { 20 };
    (base + successful_cases.min(20) as u8 * 2).min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_low_without_verified_identity() {
        assert_eq!(initial_score(false, 0), 20);
    }

    #[test]
    fn rewards_verified_identity_and_successful_cases() {
        assert_eq!(initial_score(true, 3), 66);
    }

    #[test]
    fn caps_score_at_one_hundred() {
        assert_eq!(initial_score(true, 200), 100);
    }
}
