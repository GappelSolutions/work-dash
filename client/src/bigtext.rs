//! Minimal 3x5 block-digit font for the clock, rendered double-width so the
//! digits look roughly square in a terminal cell grid.

fn glyph(c: char) -> [&'static str; 5] {
    match c {
        '0' => ["███", "█ █", "█ █", "█ █", "███"],
        '1' => [" █ ", "██ ", " █ ", " █ ", "███"],
        '2' => ["███", "  █", "███", "█  ", "███"],
        '3' => ["███", "  █", "███", "  █", "███"],
        '4' => ["█ █", "█ █", "███", "  █", "  █"],
        '5' => ["███", "█  ", "███", "  █", "███"],
        '6' => ["███", "█  ", "███", "█ █", "███"],
        '7' => ["███", "  █", "  █", "  █", "  █"],
        '8' => ["███", "█ █", "███", "█ █", "███"],
        '9' => ["███", "█ █", "███", "  █", "███"],
        ':' => [" ", "█", " ", "█", " "],
        'B' => ["██ ", "█ █", "██ ", "█ █", "██ "],
        'R' => ["██ ", "█ █", "██ ", "█ █", "█ █"],
        'E' => ["███", "█  ", "██ ", "█  ", "███"],
        'A' => ["███", "█ █", "███", "█ █", "█ █"],
        'K' => ["█ █", "██ ", "█  ", "██ ", "█ █"],
        'T' => ["███", " █ ", " █ ", " █ ", " █ "],
        'I' => ["███", " █ ", " █ ", " █ ", "███"],
        'M' => ["█ █", "███", "█ █", "█ █", "█ █"],
        _ => ["   ", "   ", "   ", "   ", "   "],
    }
}

/// Render `s` as 5 lines of doubled block characters.
pub fn big_lines(s: &str) -> Vec<String> {
    let mut lines = vec![String::new(); 5];
    for (gi, c) in s.chars().enumerate() {
        let g = glyph(c);
        for (row, line) in lines.iter_mut().enumerate() {
            if gi > 0 {
                line.push_str("  ");
            }
            for pc in g[row].chars() {
                line.push_str(if pc == '█' { "██" } else { "  " });
            }
        }
    }
    lines
}

/// Display width in terminal columns of `big_lines(s)`.
pub fn width(s: &str) -> u16 {
    big_lines(s)
        .first()
        .map(|l| l.chars().count() as u16)
        .unwrap_or(0)
}
