pub fn haversine_km(lat_a: f64, lng_a: f64, lat_b: f64, lng_b: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;

    let lat_a = lat_a.to_radians();
    let lat_b = lat_b.to_radians();
    let delta_lat = lat_b - lat_a;
    let delta_lng = (lng_b - lng_a).to_radians();

    let a = (delta_lat / 2.0).sin().powi(2)
        + lat_a.cos() * lat_b.cos() * (delta_lng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_KM * c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_zero_for_same_point() {
        assert_eq!(haversine_km(-23.5505, -46.6333, -23.5505, -46.6333), 0.0);
    }

    #[test]
    fn calculates_distance_between_sao_paulo_and_rio() {
        let distance = haversine_km(-23.5505, -46.6333, -22.9068, -43.1729);

        assert!((350.0..370.0).contains(&distance));
    }
}
