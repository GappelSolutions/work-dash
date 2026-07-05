//! Inline SVG icons — vendored from Lucide (ISC license, see assets/icons/LICENSE).
//! Inlined (not <img src>) so `stroke="currentColor"` picks up the surrounding
//! CSS text color instead of rendering black.

use maud::{html, Markup, PreEscaped};

macro_rules! icon_svg {
    ($name:literal) => {
        include_str!(concat!("../assets/icons/", $name, ".svg"))
    };
}

pub fn icon(name: &str) -> Markup {
    let svg = match name {
        "calendar" => icon_svg!("calendar"),
        "calendar-days" => icon_svg!("calendar-days"),
        "chevron-left" => icon_svg!("chevron-left"),
        "chevron-right" => icon_svg!("chevron-right"),
        "phone-incoming" => icon_svg!("phone-incoming"),
        "alarm-clock" => icon_svg!("alarm-clock"),
        "plug" => icon_svg!("plug"),
        "plus" => icon_svg!("plus"),
        "more-horizontal" => icon_svg!("more-horizontal"),
        "trash-2" => icon_svg!("trash-2"),
        "pencil" => icon_svg!("pencil"),
        "log-out" => icon_svg!("log-out"),
        "x" => icon_svg!("x"),
        "check" => icon_svg!("check"),
        other => panic!("unknown icon: {other}"),
    };
    html! { span class=(format!("icon icon-{name}")) { (PreEscaped(svg)) } }
}
