/// Integer Bresenham line points, including both endpoints.
pub fn line_points(from: (i32, i32), to: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let dy = -(to.1 - y).abs();
    let sy = if y < to.1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut done = false;

    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let point = (x, y);
        if point == to {
            done = true;
            return Some(point);
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        Some(point)
    })
}
