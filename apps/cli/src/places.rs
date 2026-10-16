//! Automatic placement: spread loci along a readable route over the stage.

/// Positions (0..1) for `n` loci on a serpentine route: left to right, then
/// right to left one row lower, and so on, with a margin around the edge.
pub fn route(n: usize) -> Vec<(f64, f64)> {
    if n == 0 {
        return Vec::new();
    }
    let per_row = n.clamp(1, 5);
    let rows = n.div_ceil(per_row);
    let (margin_x, margin_y) = (0.12, 0.18);
    let mut points = Vec::with_capacity(n);
    for i in 0..n {
        let (row, mut col) = (i / per_row, i % per_row);
        let in_row = if row == rows - 1 {
            n - row * per_row
        } else {
            per_row
        };
        if row % 2 == 1 {
            col = in_row - 1 - col;
        }
        let x = if in_row == 1 {
            0.5
        } else {
            margin_x + (1.0 - 2.0 * margin_x) * col as f64 / (in_row - 1) as f64
        };
        let y = if rows == 1 {
            0.5
        } else {
            margin_y + (1.0 - 2.0 * margin_y) * row as f64 / (rows - 1) as f64
        };
        points.push(((x * 1000.0).round() / 1000.0, (y * 1000.0).round() / 1000.0));
    }
    points
}

#[cfg(test)]
mod tests {
    use super::route;

    #[test]
    fn routes_stay_inside_and_snake() {
        assert!(route(0).is_empty());
        assert_eq!(route(1), [(0.5, 0.5)]);
        let r = route(7);
        assert_eq!(r.len(), 7);
        assert!(
            r.iter()
                .all(|&(x, y)| (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
        );
        // second row runs right to left
        assert!(r[5].0 > r[6].0);
        assert!(r[0].1 < r[5].1);
    }
}
