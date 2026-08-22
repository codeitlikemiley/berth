/// Screenshot-pixel coordinate, origin top-left.
pub type Point = [i32; 2];

/// Axis-aligned box in screenshot pixel space: `[x, y, x2, y2]` -- top-left and
/// bottom-right, not width/height. This was previously documented as either,
/// which is not a thing two implementations can agree on.
pub type Region = [i32; 4];

/// Map a point from last-frame pixel space into guest/input pixel space.
///
/// Protocol coordinates are always in the pixel space of the last full frame,
/// origin top-left. If a screenshot was scaled down before sending, incoming
/// click/move/drag coordinates must be scaled back up before injecting input.
#[must_use]
pub fn scale_coordinates(
    xy: Point,
    from_width: u32,
    from_height: u32,
    to_width: u32,
    to_height: u32,
) -> Point {
    if from_width == 0 || from_height == 0 {
        return xy;
    }
    [
        scale_axis(xy[0], from_width, to_width),
        scale_axis(xy[1], from_height, to_height),
    ]
}

/// Scale an `[x, y, x2, y2]` box by scaling each corner as a point.
#[must_use]
pub fn scale_region(
    region: Region,
    from_width: u32,
    from_height: u32,
    to_width: u32,
    to_height: u32,
) -> Region {
    let a = scale_coordinates(
        [region[0], region[1]],
        from_width,
        from_height,
        to_width,
        to_height,
    );
    let b = scale_coordinates(
        [region[2], region[3]],
        from_width,
        from_height,
        to_width,
        to_height,
    );
    [a[0], a[1], b[0], b[1]]
}

fn scale_axis(v: i32, from: u32, to: u32) -> i32 {
    let from = i64::from(from);
    let to = i64::from(to);
    let v = i64::from(v);
    // Round to nearest so a center pixel stays a center pixel after integer scale.
    let rounded = if v >= 0 {
        (v * to + from / 2) / from
    } else {
        (v * to - from / 2) / from
    };
    rounded as i32
}
