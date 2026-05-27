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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_zero_for_neutral_text() {
        assert_eq!(
            score_post_text("Animal vacinado para adoção responsavel"),
            0
        );
    }

    #[test]
    fn scores_known_risk_markers() {
        let score = score_post_text("Urgente, mando pix de recompensa fora da plataforma");

        assert_eq!(score, 60);
    }

    #[test]
    fn caps_score_at_one_hundred() {
        let repeated = "pix urgente recompensa fora da plataforma ".repeat(10);

        assert!(score_post_text(&repeated) <= 100);
    }
}
