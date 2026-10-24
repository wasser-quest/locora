//! The share card: a square picture (1080 × 1080) drawn on a canvas that is never shown. The
//! medal comes as a PNG from the core (`share_art`); the texts are drawn with the device's own
//! fonts, so every language shows, and a text drawn on a canvas can never become markup. The
//! colours are the light palette's, whatever the app's theme.

use wasm_bindgen::JsCast;

use crate::share::CardText;

/// The card's side, in pixels.
pub const SIDE: u32 = 1080;
const FONT: &str = "system-ui, -apple-system, \"Segoe UI\", sans-serif";
const BACKGROUND: &str = "#f3eee3";
const SURFACE: &str = "#fffaf0";
const TEXT: &str = "#22234f";
const MUTED: &str = "#5f6089";
const INDIGO: &str = "#3a3d92";
const LIME: &str = "#c9e85a";

/// `text` broken into lines no wider than `width`: by words, and a word too long for a line (and
/// text without spaces, such as Chinese or Japanese) by characters. At most `lines` lines; the last
/// ends with „…“ when something was left out.
pub fn wrap(text: &str, width: f64, lines: usize, measure: impl Fn(&str) -> f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    // Character by character: a line that is full goes out, and the next one starts.
    let place = |line: &mut String, piece: &str, out: &mut Vec<String>| {
        for c in piece.chars() {
            let mut longer = line.clone();
            longer.push(c);
            if measure(&longer) > width && !line.is_empty() {
                out.push(std::mem::take(line));
                if c == ' ' {
                    continue;
                }
            }
            line.push(c);
        }
    };
    for (index, word) in text.split(' ').filter(|word| !word.is_empty()).enumerate() {
        let joined = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if measure(&joined) <= width {
            line = joined;
        } else if index > 0 && !line.is_empty() && measure(word) <= width {
            out.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            // Too long for a line of its own (or a script without spaces): by characters.
            let piece = if line.is_empty() {
                word.to_owned()
            } else {
                format!(" {word}")
            };
            place(&mut line, &piece, &mut out);
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    if out.len() > lines {
        out.truncate(lines);
        if let Some(last) = out.last_mut() {
            let mut cut = last.trim_end().to_owned();
            while !cut.is_empty() && measure(&format!("{cut}…")) > width {
                cut.pop();
            }
            *last = format!("{}…", cut.trim_end());
        }
    }
    out
}

fn image(png: &str) -> Result<web_sys::HtmlImageElement, String> {
    let image = web_sys::HtmlImageElement::new().map_err(|_| "image".to_owned())?;
    image.set_src(&format!("data:image/png;base64,{png}"));
    Ok(image)
}

async fn decoded(image: &web_sys::HtmlImageElement) -> Result<(), String> {
    wasm_bindgen_futures::JsFuture::from(image.decode())
        .await
        .map(drop)
        .map_err(|_| "image".to_owned())
}

/// A rectangle with round corners, as a path (arcTo, which older WebKitGTK knows too).
fn rounded(context: &web_sys::CanvasRenderingContext2d, x: f64, y: f64, side: f64, radius: f64) {
    let end = x + side;
    let bottom = y + side;
    context.begin_path();
    context.move_to(x + radius, y);
    let _ = context.arc_to(end, y, end, bottom, radius);
    let _ = context.arc_to(end, bottom, x, bottom, radius);
    let _ = context.arc_to(x, bottom, x, y, radius);
    let _ = context.arc_to(x, y, end, y, radius);
    context.close_path();
}

/// Draws the card and returns it as a PNG data URL. `art_png` and `logo_png` are PNGs (base64)
/// from `share_art`.
pub async fn render(
    text: &CardText,
    art_png: &str,
    logo_png: Option<&str>,
) -> Result<String, String> {
    let failed = |_| crate::i18n::t("Das Bild konnte nicht erstellt werden.").to_owned();
    let document = web_sys::window()
        .and_then(|window| window.document())
        .ok_or_else(|| failed(()))?;
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|_| failed(()))?
        .dyn_into()
        .map_err(|_| failed(()))?;
    canvas.set_width(SIDE);
    canvas.set_height(SIDE);
    let context: web_sys::CanvasRenderingContext2d = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|context| context.dyn_into().ok())
        .ok_or_else(|| failed(()))?;
    let art = image(art_png)?;
    decoded(&art).await.map_err(|_| failed(()))?;
    let logo = match logo_png {
        Some(png) => {
            let logo = image(png)?;
            decoded(&logo).await.ok().map(|()| logo)
        }
        None => None,
    };
    let side = f64::from(SIDE);
    context.set_fill_style_str(BACKGROUND);
    context.fill_rect(0.0, 0.0, side, side);
    context.set_fill_style_str(SURFACE);
    rounded(&context, 40.0, 40.0, side - 80.0, 48.0);
    context.fill();
    // The wordmark, top left.
    if let Some(logo) = &logo {
        let _ =
            context.draw_image_with_html_image_element_and_dw_and_dh(logo, 96.0, 84.0, 88.0, 88.0);
    }
    context.set_text_baseline("middle");
    context.set_text_align("left");
    context.set_fill_style_str(INDIGO);
    context.set_font(&format!("700 48px {FONT}"));
    let _ = context.fill_text("Locora", if logo.is_some() { 200.0 } else { 96.0 }, 128.0);
    // The medal, centred, at most 480 square.
    let (width, height) = (
        f64::from(art.natural_width()),
        f64::from(art.natural_height()),
    );
    if width > 0.0 && height > 0.0 {
        let scale = (480.0 / width).min(480.0 / height);
        let (w, h) = (width * scale, height * scale);
        let _ = context.draw_image_with_html_image_element_and_dw_and_dh(
            &art,
            (side - w) / 2.0,
            170.0 + (480.0 - h) / 2.0,
            w,
            h,
        );
    }
    context.set_text_align("center");
    context.set_text_baseline("alphabetic");
    let center = side / 2.0;
    let line = |font: &str, color: &str, words: &str, y: f64| {
        context.set_font(font);
        context.set_fill_style_str(color);
        let measure = |s: &str| context.measure_text(s).map(|m| m.width()).unwrap_or(0.0);
        let shown = wrap(words, 900.0, 1, measure);
        if let Some(shown) = shown.first() {
            let _ = context.fill_text(shown, center, y);
        }
    };
    line(&format!("600 40px {FONT}"), MUTED, &text.heading, 720.0);
    context.set_font(&format!("700 68px {FONT}"));
    context.set_fill_style_str(TEXT);
    let measure = |s: &str| context.measure_text(s).map(|m| m.width()).unwrap_or(0.0);
    let title = wrap(&text.title, 900.0, 2, measure);
    let mut y = 800.0;
    for part in &title {
        let _ = context.fill_text(part, center, y);
        y += 76.0;
    }
    if let Some(detail) = &text.detail {
        line(&format!("40px {FONT}"), TEXT, detail, y - 10.0);
    }
    line(&format!("36px {FONT}"), MUTED, &text.footer, 1000.0);
    context.set_fill_style_str(LIME);
    context.fill_rect(140.0, 1024.0, side - 280.0, 16.0);
    canvas
        .to_data_url_with_type("image/png")
        .map_err(|_| failed(()))
}

#[cfg(test)]
mod tests {
    use super::wrap;

    /// Every character is 30 wide.
    fn measure(text: &str) -> f64 {
        text.chars().count() as f64 * 30.0
    }

    #[test]
    fn long_titles_wrap_into_two_lines_and_end_with_an_ellipsis() {
        assert_eq!(wrap("Erste Journey", 900.0, 2, measure), ["Erste Journey"]);
        // 30 characters fit a line of 900.
        let lines = wrap(
            "Alle Techniken gelernt und noch viel mehr dazu",
            900.0,
            2,
            measure,
        );
        assert_eq!(lines, ["Alle Techniken gelernt und", "noch viel mehr dazu"]);
        // A German compound longer than a line breaks by characters.
        let compound = "Donaudampfschifffahrtsgesellschaftskapitän";
        let lines = wrap(compound, 900.0, 2, measure);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines.concat(), compound);
        assert!(lines.iter().all(|line| measure(line) <= 900.0));
        // Chinese has no spaces: by characters, and the rest is cut with an ellipsis.
        let chinese = "完成了所有的记忆宫殿练习并且每天都坚持复习所有的地点和内容直到全部记住为止还有更多的内容需要显示在这里";
        let lines = wrap(chinese, 300.0, 2, measure);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'), "{lines:?}");
        assert!(lines.iter().all(|line| measure(line) <= 300.0), "{lines:?}");
        let many = wrap("one two three four five six seven", 150.0, 2, measure);
        assert_eq!(many.len(), 2);
        assert!(
            many[1].ends_with('…') && measure(&many[1]) <= 150.0,
            "{many:?}"
        );
        assert!(wrap("", 900.0, 2, measure).is_empty());
    }
}
