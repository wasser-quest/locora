//! Activity overview in the style of a contribution graph: one square per learning day, shaded by
//! how many answers it had. Days without answers show too, so inactive stretches are visible.

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{ActivityDay, CMD_ACTIVITY};

use crate::i18n::{t, tf};
use crate::tauri::{NoArgs, call};

/// Shade of a day: 0 for no answers, 1–4 by the number of answers.
fn level(answers: u32) -> u8 {
    match answers {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        6..=9 => 3,
        _ => 4,
    }
}

/// The date of a learning day (its start in seconds) as YYYY-MM-DD, for the square's tooltip.
fn date_label(day: u64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(day as f64 * 1000.0));
    date.to_iso_string()
        .as_string()
        .unwrap_or_default()
        .chars()
        .take(10)
        .collect()
}

#[component]
pub fn Activity() -> impl IntoView {
    let days = RwSignal::new(None::<Result<Vec<ActivityDay>, String>>);
    spawn_local(async move {
        days.set(Some(
            call::<Vec<ActivityDay>>(CMD_ACTIVITY, NoArgs {}).await,
        ));
    });
    move || match days.get() {
        None => None,
        Some(Err(error)) => Some(view! { <p class="notes" role="alert">{error}</p> }.into_any()),
        Some(Ok(list)) if list.is_empty() => None,
        Some(Ok(list)) => Some(grid(list).into_any()),
    }
}

fn grid(days: Vec<ActivityDay>) -> impl IntoView {
    let active = days.iter().filter(|day| day.answers > 0).count();
    let total: u32 = days.iter().map(|day| day.answers).sum();
    let squares = days
        .into_iter()
        .map(|day| {
            let date = date_label(day.day);
            let title = if day.answers == 0 {
                tf("{0}: keine Antworten", &[&date])
            } else {
                tf("{0}: {1} Antworten", &[&date, &day.answers])
            };
            view! {
                <span class=format!("activity-day l{}", level(day.answers)) title=title></span>
            }
        })
        .collect_view();
    // The newest weeks are on the right: a narrow screen starts there, not at last year.
    let grid = NodeRef::<leptos::html::Div>::new();
    Effect::new(move |_| {
        if let Some(grid) = grid.get() {
            grid.set_scroll_left(grid.scroll_width());
        }
    });
    view! {
        <section class="card panel activity" aria-label=t("Aktivität")>
            <h2>{t("Aktivität")}</h2>
            <p class="activity-summary">{tf("{0} aktive Tage in den letzten 12 Monaten · {1} Antworten", &[&active, &total])}</p>
            <div class="activity-grid" node_ref=grid>{squares}</div>
            <p class="activity-legend">
                <span>{t("weniger")}</span>
                <span class="activity-day l0"></span>
                <span class="activity-day l1"></span>
                <span class="activity-day l2"></span>
                <span class="activity-day l3"></span>
                <span class="activity-day l4"></span>
                <span>{t("mehr")}</span>
            </p>
        </section>
    }
}
