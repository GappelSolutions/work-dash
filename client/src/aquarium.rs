//! Port of cmatsuoka/asciiquarium's classic look: multi-line fish sprites
//! with color masks, a wavy water line, seaweed, bubbles and a castle.
//! Sprites and masks are taken directly from the original Perl script
//! (https://github.com/cmatsuoka/asciiquarium), minus its `Term::Animation`
//! plumbing. Stepped on every app tick regardless of which page is showing.

use rand::prelude::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

/// One species, given as hand-mirrored right/left ASCII art pairs (matching
/// the source, not a runtime mirror) plus a parallel color mask. Mask digits
/// 1-9 pick a per-fish random color (digit 4 is always white, the eye);
/// spaces in the body are transparent, letting whatever's behind show.
struct Species {
    right_body: &'static str,
    right_mask: &'static str,
    left_body: &'static str,
    left_mask: &'static str,
}

const SPECIES: [Species; 7] = [
    Species {
        right_body: "       \\\n     ...\\..,\n\\  /'       \\\n >=     (  ' >\n/  \\      / /\n    `\"'\"'/''",
        right_mask: "       2\n     1112111\n6  11       1\n 66     7  4 5\n6  1      3 1\n    11111311",
        left_body: "      /\n  ,../...\n /       '\\  /\n< '  )     =<\n \\ \\      /  \\\n  `'\\'\"'\"'",
        left_mask: "      2\n  1112111\n 1       11  6\n5 4  7     66\n 1 3      1  6\n  11311111",
    },
    Species {
        right_body: "    \\\n\\ /--\\\n>=  (o>\n/ \\__/\n    /",
        right_mask: "    2\n6 1111\n66  745\n6 1111\n    3",
        left_body: "  /\n /--\\ /\n<o)  =<\n \\__/ \\\n  \\",
        left_mask: "  2\n 1111 6\n547  66\n 1111 6\n  3",
    },
    Species {
        right_body: "  __\n><_'>\n   '",
        right_mask: "  11\n61145\n   3",
        left_body: " __\n<'_><\n `",
        left_mask: " 11\n54116\n 3",
    },
    Species {
        right_body: "   ..\\,\n>='   ('>\n  '''/''",
        right_mask: "   1121\n661   745\n  111311",
        left_body: "  ,/..\n<')   `=<\n ``\\```",
        left_mask: "  1211\n547   166\n 113111",
    },
    Species {
        right_body: "   \\\n  / \\\n>=_('>\n  \\_/\n   /",
        right_mask: "   2\n  1 1\n661745\n  111\n   3",
        left_body: "  /\n / \\\n<')_=<\n \\_/\n  \\",
        left_mask: "  2\n 1 1\n547166\n 111\n  3",
    },
    Species {
        right_body: "  ,\\\n>=('>\n  '/",
        right_mask: "  12\n66745\n  13",
        left_body: " /,\n<')=<\n \\`",
        left_mask: " 21\n54766\n 31",
    },
    Species {
        right_body: "  __\n\\/ o\\\n/\\__/",
        right_mask: "  11\n61 41\n61111",
        left_body: " __\n/o \\/\n\\__/\\",
        left_mask: " 11\n14 16\n11116",
    },
];

const PALETTE: [Color; 10] = [
    Color::Cyan,
    Color::LightCyan,
    Color::Red,
    Color::LightRed,
    Color::Yellow,
    Color::LightYellow,
    Color::Blue,
    Color::LightBlue,
    Color::Green,
    Color::LightMagenta,
];

/// Tiled wavy water line, straight from `add_environment`.
const WATER_LINES: [&str; 4] = [
    "~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~",
    "^^^^ ^^^  ^^^   ^^^    ^^^^      ",
    "^^^^      ^^^^     ^^^    ^^     ",
    "^^      ^^^^      ^^^    ^^^^^^  ",
];

/// Castle art + color mask, straight from `add_castle` (R = flag, y = highlight).
const CASTLE_BODY: &str = "               T~~\n               |\n              /^\\\n             /   \\\n _   _   _  /     \\  _   _   _\n[ ]_[ ]_[ ]/ _   _ \\[ ]_[ ]_[ ]\n|_=__-_ =_|_[ ]_[ ]_|_=-___-__|\n | _- =  | =_ = _    |= _=   |\n |= -[]  |- = _ =    |_-=_[] |\n | =_    |= - ___    | =_ =  |\n |=  []- |-  /| |\\   |=_ =[] |\n |- =_   | =| | | |  |- = -  |\n |_______|__|_|_|_|__|_______|";
const CASTLE_MASK: &str = "                RR\n\n              yyy\n             y   y\n            y     y\n           y       y\n\n\n\n              yyy\n             yy yy\n            y y y y\n            yyyyyyy";
const CASTLE_HEIGHT: u16 = 13;

/// Shark, from `add_shark` (index 0 = swims right, 1 = swims left).
const SHARK_BODY: [&str; 2] = [
    "                              __\n                             ( `\\\n  ,                          )   `\\\n;' `.                        (     `\\__\n ;   `.             __..---''          `~~~~-._\n  `.   `.____...--''                       (b  `--._\n    >                     _.-'      .((      ._     )\n  .`.-`--...__         .-'     -.___.....-(|/|/|/|/'\n ;.'         `. ...----`.___.',,,_______......---'\n '           '-'",
    "                     __\n                    /' )\n                  /'   (                          ,\n              __/'     )                        .' `;\n      _.-~~~~'          ``---..__             .'   ;\n _.--'  b)                       ``--...____.'   .'\n(     _.      )).      `-._                     <\n `\\|\\|\\|\\|)-.....___.-     `-.         __...--'-.'.\n   `---......_______,,,`.___.'----... .'         `.;\n                                     `-`           `",
];
const SHARK_MASK: [&str; 2] = [
    "\n\n\n\n\n                                           cR\n \n                                          cWWWWWWWW\n\n",
    "\n\n\n\n\n        Rc\n\n  WWWWWWWWc\n\n",
];

/// Ship, from `add_ship`.
const SHIP_BODY: [&str; 2] = [
    "     |    |    |\n    )_)  )_)  )_)\n   )___))___))___)\\\n  )____)____)_____)\\\\\n_____|____|____|____\\\\\\__\n\\                   /",
    "         |    |    |\n        (_(  (_(  (_(\n      /(___((___((___(\n    //(_____(____(____(\n__///____|____|____|_____\n    \\                   /",
];
const SHIP_MASK: [&str; 2] = [
    "     y    y    y\n\n                  w\n                   ww\nyyyyyyyyyyyyyyyyyyyywwwyy\ny                   y",
    "         y    y    y\n\n      w\n    ww\nyywwwyyyyyyyyyyyyyyyyyyyy\n    y                   y",
];

/// Whale, from `add_whale`. The mask is authored for the union of a 3-row
/// water spout plus the 4-row body; the spout animation here is a simplified
/// stand-in for the original's frame-aligned version.
const WHALE_MASK: [&str; 2] = [
    "             C C\n           CCCCCCC\n           C  C  C\n        BBBBBBB\n      BB       BB\nB    B       BWB B\nBBBBB          BBBB",
    "   C C\n CCCCCCC\n C  C  C\n    BBBBBBB\n  BB       BB\n B BWB       B    B\nBBBB          BBBBB",
];
/// Every animation frame is the full 7-row sprite the mask expects (3 spout
/// rows, blank when idle, plus the 4-row body) so body/mask rows always
/// line up regardless of which frame is showing.
const WHALE_FRAMES: [[&str; 4]; 2] = [
    [
        "\n\n\n        .-----:\n      .'       `.\n,    /       (o) \\\n\\`._/          ,__)",
        "\n\n         :\n        .-----:\n      .'       `.\n,    /       (o) \\\n\\`._/          ,__)",
        "\n         :\n         :\n        .-----:\n      .'       `.\n,    /       (o) \\\n\\`._/          ,__)",
        "        . .\n        -:-\n         :\n        .-----:\n      .'       `.\n,    /       (o) \\\n\\`._/          ,__)",
    ],
    [
        "\n\n\n    :-----.\n  .'       `.\n / (o)       \\    ,\n(__,          \\_.'/",
        "\n\n  :\n    :-----.\n  .'       `.\n / (o)       \\    ,\n(__,          \\_.'/",
        "\n  :\n  :\n    :-----.\n  .'       `.\n / (o)       \\    ,\n(__,          \\_.'/",
        " . .\n -:-\n  :\n    :-----.\n  .'       `.\n / (o)       \\    ,\n(__,          \\_.'/",
    ],
];

/// Monster, from `add_new_monster` (2 animation frames per direction).
const MONSTER_BODY: [[&str; 2]; 2] = [
    [
        "         _   _                     _   _       _a_a\n       _{.`=`.}_      _   _      _{.`=`.}_    {/ ''\\_\n _    {.'  _  '.}    {.`'`.}    {.'  _  '.}  {|  ._oo)\n{ \\  {/  .' '.  \\}  {/ .-. \\}  {/  .' '.  \\} {/  |",
        "                      _   _                    _a_a\n  _      _   _      _{.`=`.}_      _   _      {/ ''\\_\n { \\    {.`'`.}    {.'  _  '.}    {.`'`.}    {|  ._oo)\n  \\ \\  {/ .-. \\}  {/  .' '.  \\}  {/ .-. \\}   {/  |",
    ],
    [
        "   a_a_       _   _                     _   _\n _/'' \\}    _{.`=`.}_      _   _      _{.`=`.}_\n(oo_.  |}  {.'  _  '.}    {.`'`.}    {.'  _  '.}    _\n    |  \\} {/  .' '.  \\}  {/ .-. \\}  {/  .' '.  \\}  / }",
        "   a_a_                    _   _\n _/'' \\}      _   _      _{.`=`.}_      _   _      _\n(oo_.  |}    {.`'`.}    {.'  _  '.}    {.`'`.}    / }\n    |  \\}   {/ .-. \\}  {/  .' '.  \\}  {/ .-. \\}  / /",
    ],
];
const MONSTER_MASK: [&str; 2] = [
    "                                                W W\n\n\n",
    "   W W\n\n\n",
];

/// Big fish (variant 1 of 2 from the source; the other has heavy backslash
/// escaping in the original that's too easy to mistranscribe), from
/// `add_big_fish_1`. Uses the same random-digit-color mask scheme as the
/// small fish.
const BIG_FISH_BODY: [&str; 2] = [
    " ______\n`\"\"-.  `````-----.....__\n     `.  .      .       `-.\n       :     .     .       `.\n ,     :   .    .          _ :\n: `.   :                  (@) `._\n `. `..'     .     =`-.       .__)\n   ;     .        =  ~  :     .-\"\n .' .'`.   .    .  =.-'  `._ .'\n: .'   :               .   .'\n '   .'  .    .     .   .-'\n   .'____....----''.'=.'\n   \"\"             .'.'\n               ''\"'`",
    "                           ______\n          __.....-----'''''  .-\"\"'\n       .-'       .      .  .'\n     .'       .     .     :\n    : _          .    .   :     ,\n _.' (@)                  :   .' :\n(__.       .-'=     .     `..' .'\n \"-.     :  ~  =        .     ;\n   `. _.'  `-.=  .    .   .'`. `.\n     `.   .               :   `. :\n       `-.   .     .    .  `.   `\n          `.=`.``----....____`.\n            `.`.             \"\"\n              '`\"``",
];
const BIG_FISH_MASK: [&str; 2] = [
    " 111111\n11111  11111111111111111\n     11  2      2       111\n       1     2     2       11\n 1     1   2    2          1 1\n1 11   1                  1W1 111\n 11 1111     2     1111       1111\n   1     2        1  1  1     111\n 11 1111   2    2  1111  111 11\n1 11   1               2   11\n 1   11  2    2     2   111\n   111111111111111111111\n   11             1111\n               11111",
    "                           111111\n          11111111111111111  11111\n       111       2      2  11\n     11       2     2     1\n    1 1          2    2   1     1\n 111 1W1                  1   11 1\n1111       1111     2     1111 11\n 111     1  1  1        2     1\n   11 111  1111  2    2   1111 11\n     11   2               1   11 1\n       111   2     2    2  11   1\n          111111111111111111111\n            1111             11\n              11111",
];

const SURFACE_ROWS: u16 = 4;
const MAX_BUBBLES: usize = 24;
const BUBBLE_SPAWN_PCT: u32 = 5;
const BUBBLE_MOVE_TICKS: u32 = 2;
const SEAWEED_SWAY_TICKS: u64 = 6;
const VISITOR_FRAME_TICKS: u32 = 10;

/// Fixed letter -> color mapping used by the source's non-fish sprites
/// (castle, shark, ship, whale, monster): lowercase is the dim variant,
/// uppercase the bright one, matching `rand_color`'s palette pairs.
fn letter_color(ch: char) -> Option<Color> {
    match ch {
        'c' => Some(Color::Cyan),
        'C' => Some(Color::LightCyan),
        'r' => Some(Color::Red),
        'R' => Some(Color::LightRed),
        'y' => Some(Color::Yellow),
        'Y' => Some(Color::LightYellow),
        'b' => Some(Color::Blue),
        'B' => Some(Color::LightBlue),
        'g' => Some(Color::Green),
        'G' => Some(Color::LightGreen),
        'm' => Some(Color::Magenta),
        'M' => Some(Color::LightMagenta),
        'w' => Some(Color::Gray),
        'W' => Some(Color::White),
        _ => None,
    }
}

fn mask_color(ch: char, digit_colors: &[Color; 10]) -> Option<Color> {
    ch.to_digit(10)
        .filter(|&d| (1..=9).contains(&d))
        .map(|d| digit_colors[d as usize])
}

fn random_digit_colors(rng: &mut impl Rng) -> [Color; 10] {
    let mut colors = [Color::White; 10];
    for c in colors.iter_mut() {
        *c = *PALETTE.choose(rng).unwrap();
    }
    colors[4] = Color::White; // eye highlight is always white, per the source
    colors
}

struct Fish {
    x: i32,
    y: u16,
    dir_right: bool,
    species: usize,
    digit_colors: [Color; 10],
    interval: u32,
    counter: u32,
}

impl Fish {
    fn glyphs(&self) -> (&'static str, &'static str) {
        let s = &SPECIES[self.species];
        if self.dir_right {
            (s.right_body, s.right_mask)
        } else {
            (s.left_body, s.left_mask)
        }
    }

    fn size(&self) -> (i32, u16) {
        let (body, _) = self.glyphs();
        let w = body.lines().map(|l| l.chars().count()).max().unwrap_or(0) as i32;
        let h = body.lines().count() as u16;
        (w, h)
    }

    fn spawn(w: u16, h: u16, rng: &mut impl Rng, initial: bool) -> Fish {
        let dir_right = rng.random_bool(0.5);
        let species = rng.random_range(0..SPECIES.len());
        let interval = rng.random_range(2..6);

        let body = if dir_right {
            SPECIES[species].right_body
        } else {
            SPECIES[species].left_body
        };
        let sprite_h = body.lines().count() as u16;
        let sprite_w = body.lines().map(|l| l.chars().count()).max().unwrap_or(0) as i32;

        let y_min = SURFACE_ROWS + 1;
        let y_max = h.saturating_sub(sprite_h + 1).max(y_min);
        let y = if y_max > y_min {
            y_min + rng.random_range(0..(y_max - y_min))
        } else {
            y_min
        };

        let x = if initial {
            rng.random_range(0..(w as i32).max(1))
        } else if dir_right {
            -sprite_w
        } else {
            w as i32
        };

        Fish {
            x,
            y,
            dir_right,
            species,
            digit_colors: random_digit_colors(rng),
            interval,
            counter: 0,
        }
    }
}

struct Bubble {
    x: u16,
    y: i32,
    moves: u32,
    counter: u32,
}

struct Seaweed {
    x: u16,
    height: u16,
    phase: u32,
}

/// One of the source's `random_objects` — a big attraction that occasionally
/// crosses the tank. The original always keeps exactly one on screen,
/// respawning a new random kind the instant the previous one exits.
#[derive(Clone, Copy)]
enum VisitorKind {
    Shark,
    Ship,
    Whale,
    Monster,
    BigFish,
}

struct Visitor {
    kind: VisitorKind,
    dir_right: bool,
    x: i32,
    y: u16,
    frame: usize,
    frame_counter: u32,
    move_counter: u32,
    move_interval: u32,
    digit_colors: [Color; 10],
}

impl Visitor {
    fn glyphs(&self) -> (&'static str, &'static str) {
        let idx = if self.dir_right { 0 } else { 1 };
        match self.kind {
            VisitorKind::Shark => (SHARK_BODY[idx], SHARK_MASK[idx]),
            VisitorKind::Ship => (SHIP_BODY[idx], SHIP_MASK[idx]),
            VisitorKind::Monster => (MONSTER_BODY[idx][self.frame % 2], MONSTER_MASK[idx]),
            VisitorKind::BigFish => (BIG_FISH_BODY[idx], BIG_FISH_MASK[idx]),
            VisitorKind::Whale => (WHALE_FRAMES[idx][self.frame % 4], WHALE_MASK[idx]),
        }
    }

    /// Default fill color for glyphs the mask doesn't override, matching
    /// each entity's `default_color` in the source (fish/castle aside).
    fn default_color(&self) -> Color {
        match self.kind {
            VisitorKind::Shark => Color::Cyan,
            VisitorKind::Ship | VisitorKind::Whale => Color::White,
            VisitorKind::Monster => Color::Green,
            VisitorKind::BigFish => Color::Yellow,
        }
    }

    fn size(&self) -> (i32, u16) {
        let (body, _) = self.glyphs();
        let w = body.lines().map(|l| l.chars().count()).max().unwrap_or(0) as i32;
        let h = body.lines().count() as u16;
        (w, h)
    }

    fn spawn(w: u16, h: u16, rng: &mut impl Rng) -> Visitor {
        let kind = *[
            VisitorKind::Shark,
            VisitorKind::Ship,
            VisitorKind::Whale,
            VisitorKind::Monster,
            VisitorKind::BigFish,
        ]
        .choose(rng)
        .unwrap();
        let dir_right = rng.random_bool(0.5);

        let y = match kind {
            VisitorKind::Ship => 0,
            VisitorKind::Whale => 0,
            VisitorKind::Monster => SURFACE_ROWS.min(h.saturating_sub(1)),
            VisitorKind::Shark => {
                let lo = SURFACE_ROWS + 2;
                let hi = h.saturating_sub(12).max(lo);
                if hi > lo { lo + rng.random_range(0..(hi - lo)) } else { lo }
            }
            VisitorKind::BigFish => {
                let lo = SURFACE_ROWS + 2;
                let hi = h.saturating_sub(16).max(lo);
                if hi > lo { lo + rng.random_range(0..(hi - lo)) } else { lo }
            }
        };

        let move_interval = match kind {
            VisitorKind::Shark | VisitorKind::Monster => rng.random_range(1..3),
            VisitorKind::BigFish => rng.random_range(1..3),
            VisitorKind::Ship | VisitorKind::Whale => rng.random_range(3..6),
        };

        let mut visitor = Visitor {
            kind,
            dir_right,
            x: 0,
            y,
            frame: 0,
            frame_counter: 0,
            move_counter: 0,
            move_interval,
            digit_colors: random_digit_colors(rng),
        };
        let (sprite_w, _) = visitor.size();
        visitor.x = if dir_right { -sprite_w } else { w as i32 };
        visitor
    }
}

pub struct AquariumField {
    w: u16,
    h: u16,
    fish: Vec<Fish>,
    bubbles: Vec<Bubble>,
    seaweed: Vec<Seaweed>,
    visitor: Option<Visitor>,
    tick: u64,
}

impl AquariumField {
    pub fn new(w: u16, h: u16) -> AquariumField {
        let mut rng = rand::rng();
        let (fish, seaweed, visitor) = if w > 0 && h > 0 {
            let fish_count = (w as usize / 14).clamp(3, 10);
            let fish = (0..fish_count)
                .map(|_| Fish::spawn(w, h, &mut rng, true))
                .collect();
            let weed_count = (w as usize / 10).clamp(2, 12);
            let weed = (0..weed_count)
                .map(|_| Seaweed {
                    x: rng.random_range(0..w),
                    height: rng.random_range(3..7).min(h.saturating_sub(3).max(2)),
                    phase: rng.random_range(0..2),
                })
                .collect();
            (fish, weed, Some(Visitor::spawn(w, h, &mut rng)))
        } else {
            (Vec::new(), Vec::new(), None)
        };
        AquariumField {
            w,
            h,
            fish,
            seaweed,
            visitor,
            bubbles: Vec::new(),
            tick: 0,
        }
    }

    /// Rebuild the field if the render area changed size.
    pub fn ensure_size(&mut self, w: u16, h: u16) {
        if self.w != w || self.h != h {
            *self = AquariumField::new(w, h);
        }
    }

    pub fn step(&mut self) {
        if self.w == 0 || self.h == 0 {
            return;
        }
        let mut rng = rand::rng();
        let (w, h) = (self.w, self.h);
        self.tick = self.tick.wrapping_add(1);

        for fish in &mut self.fish {
            fish.counter += 1;
            if fish.counter < fish.interval {
                continue;
            }
            fish.counter = 0;
            fish.x += if fish.dir_right { 1 } else { -1 };

            if self.bubbles.len() < MAX_BUBBLES && rng.random_range(0..100) < BUBBLE_SPAWN_PCT {
                let (sprite_w, sprite_h) = fish.size();
                let bx = if fish.dir_right {
                    fish.x + sprite_w
                } else {
                    fish.x - 1
                };
                if bx >= 0 && bx < w as i32 {
                    self.bubbles.push(Bubble {
                        x: bx as u16,
                        y: (fish.y + sprite_h / 2) as i32,
                        moves: 0,
                        counter: 0,
                    });
                }
            }

            let (sprite_w, _) = fish.size();
            let fully_off = if fish.dir_right {
                fish.x > w as i32
            } else {
                fish.x + sprite_w < 0
            };
            if fully_off {
                *fish = Fish::spawn(w, h, &mut rng, false);
            }
        }

        self.bubbles.retain_mut(|b| {
            b.counter += 1;
            if b.counter >= BUBBLE_MOVE_TICKS {
                b.counter = 0;
                b.y -= 1;
                b.moves += 1;
            }
            b.y > SURFACE_ROWS as i32 - 2
        });

        if self.tick.is_multiple_of(SEAWEED_SWAY_TICKS) {
            for weed in &mut self.seaweed {
                weed.phase = weed.phase.wrapping_add(1);
            }
        }

        if let Some(visitor) = &mut self.visitor {
            visitor.frame_counter += 1;
            if visitor.frame_counter >= VISITOR_FRAME_TICKS {
                visitor.frame_counter = 0;
                visitor.frame = visitor.frame.wrapping_add(1);
            }

            visitor.move_counter += 1;
            if visitor.move_counter >= visitor.move_interval {
                visitor.move_counter = 0;
                visitor.x += if visitor.dir_right { 1 } else { -1 };
            }

            let (sprite_w, _) = visitor.size();
            let fully_off = if visitor.dir_right {
                visitor.x > w as i32
            } else {
                visitor.x + sprite_w < 0
            };
            if fully_off {
                self.visitor = Some(Visitor::spawn(w, h, &mut rng));
            }
        }
    }
}

/// Render target clipped to the tank's logical size (which may be smaller
/// than `area` right after a resize, before the field catches up).
struct Canvas<'a> {
    buf: &'a mut Buffer,
    area: Rect,
    w: u16,
    h: u16,
}

impl Canvas<'_> {
    /// Draw `body`/`mask` (parallel multi-line strings) at `(x, y)`, skipping
    /// spaces (transparent) and falling back to the terminal's default fg
    /// when the mask has no digit at that position.
    fn draw_sprite(
        &mut self,
        x: i32,
        y: u16,
        body: &str,
        mask: &str,
        color_of_digit: impl Fn(char) -> Option<Color>,
    ) {
        let mask_lines: Vec<&str> = mask.lines().collect();
        for (row, line) in body.lines().enumerate() {
            let py = y + row as u16;
            if py >= self.h {
                continue;
            }
            let mask_line = mask_lines.get(row).copied().unwrap_or("");
            let mask_chars: Vec<char> = mask_line.chars().collect();
            for (col, ch) in line.chars().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let px = x + col as i32;
                if px < 0 || px >= self.w as i32 {
                    continue;
                }
                let Some(cell) = self.buf.cell_mut((self.area.x + px as u16, self.area.y + py))
                else {
                    continue;
                };
                cell.set_char(ch);
                // A mask line shorter than the body line (or blank) means
                // "no override here", same as an explicit space in the mask.
                let mask_ch = mask_chars.get(col).copied().unwrap_or(' ');
                if let Some(color) = color_of_digit(mask_ch) {
                    cell.set_fg(color);
                }
            }
        }
    }
}

impl Widget for &AquariumField {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = self.w.min(area.width);
        let h = self.h.min(area.height);
        if w == 0 || h == 0 {
            return;
        }
        let mut canvas = Canvas { buf, area, w, h };

        // Wavy water line tiled across the top of the tank.
        for (row, pattern) in WATER_LINES.iter().enumerate() {
            let row = row as u16;
            if row >= h {
                break;
            }
            let bytes = pattern.as_bytes();
            for x in 0..w {
                let ch = bytes[x as usize % bytes.len()] as char;
                if let Some(cell) = canvas.buf.cell_mut((area.x + x, area.y + row)) {
                    cell.set_char(ch);
                    cell.set_fg(Color::Cyan);
                }
            }
        }

        // Castle, bottom-right of the tank.
        let castle_width = CASTLE_BODY.lines().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        if w >= castle_width + 2 && h >= CASTLE_HEIGHT + SURFACE_ROWS {
            let cx = (w - castle_width - 1) as i32;
            let cy = h - CASTLE_HEIGHT;
            canvas.draw_sprite(cx, cy, CASTLE_BODY, CASTLE_MASK, |ch| {
                Some(letter_color(ch).unwrap_or(Color::White))
            });
        }

        // Seaweed, rooted at the bottom.
        if h > SURFACE_ROWS + 1 {
            let floor = h - 1;
            for weed in &self.seaweed {
                if weed.x >= w {
                    continue;
                }
                for r in 0..weed.height.min(floor) {
                    let row = floor - r;
                    let ch = if (r as u32 + weed.phase).is_multiple_of(2) { '(' } else { ')' };
                    if let Some(cell) = canvas.buf.cell_mut((area.x + weed.x, area.y + row)) {
                        cell.set_char(ch);
                        cell.set_fg(Color::Green);
                    }
                }
            }
        }

        // Occasional big visitor: shark, ship, whale, monster or big fish.
        if let Some(visitor) = &self.visitor {
            let (body, mask) = visitor.glyphs();
            let default = visitor.default_color();
            match visitor.kind {
                VisitorKind::BigFish => {
                    let digit_colors = visitor.digit_colors;
                    canvas.draw_sprite(visitor.x, visitor.y, body, mask, move |ch| {
                        if ch == 'W' {
                            Some(Color::White)
                        } else {
                            mask_color(ch, &digit_colors)
                        }
                    });
                }
                _ => {
                    canvas.draw_sprite(visitor.x, visitor.y, body, mask, move |ch| {
                        Some(letter_color(ch).unwrap_or(default))
                    });
                }
            }
        }

        // Bubbles rising toward the surface.
        for b in &self.bubbles {
            if b.x >= w || b.y < 0 || b.y as u16 >= h {
                continue;
            }
            let ch = match b.moves {
                0 => '.',
                1 => 'o',
                _ => 'O',
            };
            if let Some(cell) = canvas.buf.cell_mut((area.x + b.x, area.y + b.y as u16)) {
                cell.set_char(ch);
                cell.set_fg(Color::LightCyan);
            }
        }

        // Fish on top of everything else.
        for fish in &self.fish {
            let (body, mask) = fish.glyphs();
            canvas.draw_sprite(fish.x, fish.y, body, mask, |ch| {
                mask_color(ch, &fish.digit_colors)
            });
        }
    }
}
