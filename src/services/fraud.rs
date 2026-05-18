pub fn score_post_text(text: &str) -> u8 {
    let lower = text.to_lowercase();
    let mut score = 0;
    for marker in ["pix", "urgente", "recompensa", "fora da plataforma"] {
        if lower.contains(marker) {
            score += 15;
        }
    }
    score.min(100)
}
