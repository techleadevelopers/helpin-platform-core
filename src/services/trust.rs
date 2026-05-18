pub fn initial_score(verified_identity: bool, successful_cases: u32) -> u8 {
    let base = if verified_identity { 60 } else { 20 };
    (base + successful_cases.min(20) as u8 * 2).min(100)
}
