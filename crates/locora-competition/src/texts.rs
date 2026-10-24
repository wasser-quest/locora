//! Texts the runtime sends as system notifications (spec §12.13), in the five
//! languages of the app. The UI's own texts live in its catalog; these are here
//! because the runtime schedules notifications without the UI.
//!
//! Terms, to match the app: daily challenge = Tages-Challenge, défi du jour,
//! デイリーチャレンジ, 每日挑战; evening recall = Abendabfrage, rappel du soir,
//! 夜のおさらい, 晚间回忆; place = Ort, lieu, か所, 位置; route = Route, trajet,
//! ルート, 路线.

use crate::schedule::{Daily, places};
use crate::{Discipline, Lang};

/// The discipline's name inside a sentence: "shopping list", "numbers", "cards".
pub fn discipline(lang: Lang, discipline: Discipline) -> &'static str {
    lang.pick(match discipline {
        Discipline::List => [
            "shopping list",
            "Einkaufsliste",
            "liste de courses",
            "買い物リスト",
            "购物清单",
        ],
        Discipline::Numbers => ["numbers", "Zahlen", "chiffres", "数字", "数字"],
        Discipline::Cards => ["cards", "Karten", "cartes", "トランプ", "扑克牌"],
    })
}

/// Title and body of the morning announcement of a daily challenge. `sizes`
/// are the items of levels 1, 2 and 3; the route it names fits level 1.
pub fn morning(lang: Lang, daily: &Daily, sizes: [u8; 3]) -> (String, String) {
    let title = if daily.long_day {
        lang.pick([
            "Daily challenge – long day",
            "Tages-Challenge – langer Tag",
            "Défi du jour – sur toute la journée",
            "デイリーチャレンジ – 一日がかり",
            "每日挑战 – 全天版",
        ])
    } else {
        lang.pick([
            "Daily challenge",
            "Tages-Challenge",
            "Défi du jour",
            "デイリーチャレンジ",
            "每日挑战",
        ])
    };
    let template = lang.pick(match daily.discipline {
        Discipline::List => [
            "Today: a shopping list ({0}, {1} or {2} things). A route with {0} places is enough for the first level.",
            "Heute: eine Einkaufsliste ({0}, {1} oder {2} Dinge). Für die erste Stufe reicht eine Route mit {0} Orten.",
            "Aujourd’hui\u{202f}: une liste de courses ({0}, {1} ou {2} articles). Pour le premier niveau, un trajet de {0} lieux suffit.",
            "今日は買い物リスト（{0}・{1}・{2} 品）。最初のレベルなら {0} か所のルートで足ります。",
            "今天：购物清单（{0}、{1} 或 {2} 件）。第一级只需一条有 {0} 个位置的路线。",
        ],
        Discipline::Numbers => [
            "Today: numbers ({0}, {1} or {2} digits). With two digits per place, {3} places are enough for the first level.",
            "Heute: Zahlen ({0}, {1} oder {2} Ziffern). Mit zwei Ziffern pro Ort reichen für die erste Stufe {3} Orte.",
            "Aujourd’hui\u{202f}: des chiffres ({0}, {1} ou {2}). Avec deux chiffres par lieu, {3} lieux suffisent pour le premier niveau.",
            "今日は数字（{0}・{1}・{2} 桁）。1 か所に 2 桁ずつ置けば、最初のレベルは {3} か所で足ります。",
            "今天：数字（{0}、{1} 或 {2} 位）。每个位置放两位数字，第一级只需 {3} 个位置。",
        ],
        Discipline::Cards => [
            "Today: cards ({0}, {1} or {2}). With two cards per place, {3} places are enough for the first level.",
            "Heute: Karten ({0}, {1} oder {2}). Mit zwei Karten pro Ort reichen für die erste Stufe {3} Orte.",
            "Aujourd’hui\u{202f}: des cartes ({0}, {1} ou {2}). Avec deux cartes par lieu, {3} lieux suffisent pour le premier niveau.",
            "今日はトランプ（{0}・{1}・{2} 枚）。1 か所に 2 枚ずつ置けば、最初のレベルは {3} か所で足ります。",
            "今天：扑克牌（{0}、{1} 或 {2} 张）。每个位置放两张牌，第一级只需 {3} 个位置。",
        ],
    });
    let mut body = fill(
        template,
        &[sizes[0], sizes[1], sizes[2], places(sizes[0], 2)],
    );
    if daily.long_day {
        body.push_str(lang.pick([
            " Memorize this morning, recall it again tonight.",
            " Morgens einprägen, abends noch einmal abrufen.",
            " À mémoriser ce matin, à retrouver ce soir.",
            "朝に覚えて、夜にもう一度思い出しましょう。",
            "早上记住，晚上再回忆一次。",
        ]));
    }
    (title.to_string(), body)
}

/// Title and body of the reminder for an evening recall of `items` things.
pub fn evening(lang: Lang, discipline: Discipline, items: u8) -> (String, String) {
    let title = lang.pick([
        "Evening recall",
        "Abendabfrage",
        "Rappel du soir",
        "夜のおさらい",
        "晚间回忆",
    ]);
    let template = lang.pick(match discipline {
        Discipline::List => [
            "Your shopping list of {0} things from earlier today: what is still there?",
            "Deine Einkaufsliste mit {0} Dingen von vorhin: Was ist noch da?",
            "Ta liste de courses de {0} articles de tout à l’heure\u{202f}: que t’en reste-t-il\u{202f}?",
            "今日覚えた買い物リスト（{0} 品）、まだ覚えていますか？",
            "你今天早些时候记住的购物清单（{0} 件）：还记得多少？",
        ],
        Discipline::Numbers => [
            "Your {0} digits from earlier today: what is still there?",
            "Deine {0} Ziffern von vorhin: Was ist noch da?",
            "Tes {0} chiffres de tout à l’heure\u{202f}: que t’en reste-t-il\u{202f}?",
            "今日覚えた {0} 桁の数字、まだ覚えていますか？",
            "你今天早些时候记住的 {0} 位数字：还记得多少？",
        ],
        Discipline::Cards => [
            "Your {0} cards from earlier today: what is still there?",
            "Deine {0} Karten von vorhin: Was ist noch da?",
            "Tes {0} cartes de tout à l’heure\u{202f}: que t’en reste-t-il\u{202f}?",
            "今日覚えた {0} 枚のトランプ、まだ覚えていますか？",
            "你今天早些时候记住的 {0} 张牌：还记得多少？",
        ],
    });
    (title.to_string(), fill(template, &[items]))
}

/// Replaces `{0}`, `{1}` … with the numbers in `args`.
fn fill(template: &str, args: &[u8]) -> String {
    let mut text = template.to_string();
    for (i, arg) in args.iter().enumerate() {
        text = text.replace(&format!("{{{i}}}"), &arg.to_string());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Date, Level, Spec, schedule};

    fn sizes(discipline: Discipline) -> [u8; 3] {
        Level::ALL.map(|l| Spec::standard(discipline, l).items)
    }

    #[test]
    fn morning_texts_for_every_discipline_and_language() {
        for lang in Lang::ALL {
            let mut bodies = Vec::new();
            for discipline in Discipline::ALL {
                for long_day in [false, true] {
                    let daily = Daily {
                        discipline,
                        long_day,
                    };
                    let (title, body) = morning(lang, &daily, sizes(discipline));
                    assert!(!title.is_empty() && !body.is_empty());
                    assert!(
                        !title.contains('{') && !body.contains('{'),
                        "{lang:?}: {body}"
                    );
                    assert!(body.contains(&sizes(discipline)[2].to_string()), "{body}");
                    bodies.push(body);
                }
            }
            let count = bodies.len();
            bodies.sort();
            bodies.dedup();
            assert_eq!(bodies.len(), count, "{lang:?}");
        }
    }

    #[test]
    fn the_english_masters() {
        let monday = schedule::daily(Date::parse("2026-10-26").unwrap());
        assert_eq!(
            morning(Lang::En, &monday, sizes(Discipline::List)),
            (
                "Daily challenge".to_string(),
                "Today: a shopping list (10, 20 or 30 things). A route with 10 places is enough for the first level.".to_string()
            )
        );
        let sunday = Daily {
            discipline: Discipline::Cards,
            long_day: true,
        };
        let (title, body) = morning(Lang::En, &sunday, sizes(Discipline::Cards));
        assert_eq!(title, "Daily challenge – long day");
        assert_eq!(
            body,
            "Today: cards (13, 26 or 52). With two cards per place, 7 places are enough for the first level. Memorize this morning, recall it again tonight."
        );
        let numbers = Daily {
            discipline: Discipline::Numbers,
            long_day: false,
        };
        assert_eq!(
            morning(Lang::De, &numbers, sizes(Discipline::Numbers)).1,
            "Heute: Zahlen (20, 40 oder 80 Ziffern). Mit zwei Ziffern pro Ort reichen für die erste Stufe 10 Orte."
        );
        assert_eq!(
            evening(Lang::En, Discipline::Cards, 26),
            (
                "Evening recall".to_string(),
                "Your 26 cards from earlier today: what is still there?".to_string()
            )
        );
    }

    #[test]
    fn evening_texts_for_every_discipline_and_language() {
        for lang in Lang::ALL {
            for discipline in Discipline::ALL {
                let (title, body) = evening(lang, discipline, 20);
                assert!(!title.is_empty() && body.contains("20"), "{body}");
                assert!(!body.contains('{'), "{body}");
            }
            let names = Discipline::ALL.map(|d| super::discipline(lang, d));
            assert!(names.iter().all(|n| !n.is_empty()));
            assert!(names[0] != names[1] && names[1] != names[2] && names[0] != names[2]);
        }
    }
}
