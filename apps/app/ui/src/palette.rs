//! Command palette (Ctrl/⌘+K): type to find a palace or a command, arrows
//! to choose, Enter to run, Escape to close.

use crate::i18n::t;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{CMD_HOME, Home, JourneyStart};
use web_sys::PointerEvent;

use crate::Screen;
use crate::tauri::{NoArgs, call};

/// Set while the editor holds unsaved changes; the palette then does not
/// navigate away.
#[derive(Clone, Copy)]
pub struct Editing(pub RwSignal<bool>);

#[derive(Debug, Clone, PartialEq)]
struct Command {
    label: String,
    hint: &'static str,
    target: Screen,
}

fn commands(home: &Home) -> Vec<Command> {
    let mut list = vec![
        Command {
            label: t("Weiter – fällige Loci wiederholen").into(),
            hint: "Journey",
            target: Screen::Journey(JourneyStart {
                due_only: true,
                ..Default::default()
            }),
        },
        Command {
            label: t("Übersicht").into(),
            hint: "Start",
            target: Screen::Home,
        },
        Command {
            label: t("Einstellungen & Statistik").into(),
            hint: "⚙",
            target: Screen::Settings,
        },
    ];
    for p in &home.palaces {
        list.push(Command {
            label: crate::i18n::tf("{0} öffnen", &[&p.name]),
            hint: "Palast",
            target: Screen::Palace(p.id.clone()),
        });
        list.push(Command {
            label: crate::i18n::tf("{0} üben", &[&p.name]),
            hint: "▶ Route",
            target: Screen::Journey(JourneyStart {
                palace_id: Some(p.id.clone()),
                ..Default::default()
            }),
        });
    }
    list
}

/// Every word of the query appears in the label (case-insensitive).
fn matches(label: &str, query: &str) -> bool {
    let label = label.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| label.contains(word))
}

#[derive(Clone, Copy)]
struct SwipeStart {
    pointer: i32,
    x: f64,
    y: f64,
    time_ms: f64,
    scroll_y: f64,
}

fn is_palette_swipe(
    start: SwipeStart,
    end_x: f64,
    end_y: f64,
    elapsed_ms: f64,
    scroll_y: f64,
) -> bool {
    let down = end_y - start.y;
    down >= 88.0
        && (end_x - start.x).abs() <= down * 0.55
        && (0.0..=900.0).contains(&elapsed_ms)
        && (scroll_y - start.scroll_y).abs() < 2.0
}

fn swipe_may_start(event: &PointerEvent) -> bool {
    if event.pointer_type() != "touch" || !event.is_primary() || event.client_y() > 64 {
        return false;
    }
    let interactive = event.target()
        .and_then(|target| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(target).ok())
        .and_then(|element| element.closest(
            "button, a, input, textarea, select, canvas, video, [contenteditable], [role='textbox'], [role='slider']"
        ).ok().flatten())
        .is_some();
    !interactive
}

#[component]
pub fn Palette(screen: RwSignal<Screen>, editing: Editing) -> impl IntoView {
    let open = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let selected = RwSignal::new(0usize);
    let all = RwSignal::new(Vec::<Command>::new());
    let note = RwSignal::new(String::new());
    let input = NodeRef::<leptos::html::Input>::new();
    // Counts openings: only the latest request may fill the list.
    let generation = StoredValue::new(0u64);
    // Where focus was before opening; it returns there on close.
    let before = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let close = move || {
        open.set(false);
        if let Some(el) = before.get_value() {
            let _ = el.focus();
        }
    };
    // Focus the search field as soon as it is there, not after loading.
    Effect::new(move |_| {
        if open.get()
            && let Some(el) = input.get()
        {
            let _ = el.focus();
        }
    });

    let shown = Memo::new(move |_| {
        let q = query.get();
        all.get()
            .into_iter()
            .filter(|c| matches(&c.label, &q))
            .take(12)
            .collect::<Vec<_>>()
    });
    let show = move || {
        let focused = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.active_element())
            .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok());
        before.set_value(focused);
        query.set(String::new());
        selected.set(0);
        note.set(String::new());
        open.set(true);
        // Static commands right away; palaces when the newest request answers.
        all.set(commands(&Home::default()));
        let this = generation.get_value() + 1;
        generation.set_value(this);
        spawn_local(async move {
            if let Ok(home) = call::<Home>(CMD_HOME, NoArgs {}).await
                && generation.get_value() == this
            {
                all.set(commands(&home));
            }
        });
    };
    let run = move |command: Command| {
        if editing.0.get_untracked() {
            note.set(t("Erst die Änderungen im Editor speichern oder verwerfen.").into());
            return;
        }
        open.set(false);
        before.set_value(None);
        screen.set(command.target);
    };

    let keys = window_event_listener(ev::keydown, move |e| {
        if (e.ctrl_key() || e.meta_key()) && e.key().eq_ignore_ascii_case("k") {
            e.prevent_default();
            if open.get_untracked() {
                close();
            } else {
                show();
            }
            return;
        }
        if !open.get_untracked() {
            return;
        }
        let count = shown.with_untracked(Vec::len);
        match e.key().as_str() {
            "Escape" => close(),
            // The search field is the dialog's only stop: Tab stays inside.
            "Tab" => {
                e.prevent_default();
                if let Some(el) = input.get_untracked() {
                    let _ = el.focus();
                }
            }
            "ArrowDown" if count > 0 => {
                e.prevent_default();
                selected.update(|i| *i = (*i + 1) % count);
            }
            "ArrowUp" if count > 0 => {
                e.prevent_default();
                selected.update(|i| *i = (*i + count - 1) % count);
            }
            "Enter" => {
                e.prevent_default();
                let pick = shown.with_untracked(|list| list.get(selected.get_untracked()).cloned());
                if let Some(command) = pick {
                    run(command);
                }
            }
            _ => {}
        }
    });
    let swipe = StoredValue::new(None::<SwipeStart>);
    let pointer_down = window_event_listener(ev::pointerdown, move |event: PointerEvent| {
        swipe.set_value(None);
        if open.get_untracked() || !swipe_may_start(&event) {
            return;
        }
        let scroll_y = web_sys::window()
            .and_then(|window| window.scroll_y().ok())
            .unwrap_or(f64::INFINITY);
        if scroll_y > 1.0 {
            return;
        }
        swipe.set_value(Some(SwipeStart {
            pointer: event.pointer_id(),
            x: f64::from(event.client_x()),
            y: f64::from(event.client_y()),
            time_ms: js_sys::Date::now(),
            scroll_y,
        }));
    });
    let pointer_up = window_event_listener(ev::pointerup, move |event: PointerEvent| {
        let start = swipe.get_value();
        swipe.set_value(None);
        let Some(start) = start.filter(|start| start.pointer == event.pointer_id()) else {
            return;
        };
        let scroll_y = web_sys::window()
            .and_then(|window| window.scroll_y().ok())
            .unwrap_or(f64::INFINITY);
        if !open.get_untracked()
            && is_palette_swipe(
                start,
                f64::from(event.client_x()),
                f64::from(event.client_y()),
                js_sys::Date::now() - start.time_ms,
                scroll_y,
            )
        {
            show();
        }
    });
    let pointer_cancel = window_event_listener(ev::pointercancel, move |event: PointerEvent| {
        if swipe
            .get_value()
            .is_some_and(|start| start.pointer == event.pointer_id())
        {
            swipe.set_value(None);
        }
    });
    on_cleanup(move || {
        keys.remove();
        pointer_down.remove();
        pointer_up.remove();
        pointer_cancel.remove();
    });

    view! {
        <Show when=move || !open.get()>
            <button class="palette-mobile-trigger" type="button" aria-label=t("Befehlspalette öffnen")
                on:click=move |_| show()>{t("Befehle")}</button>
        </Show>
        <Show when=move || open.get()>
            <div class="palette-backdrop" on:click=move |_| close()>
                <div
                    class="palette card"
                    role="dialog"
                    aria-modal="true"
                    aria-label=t("Befehlspalette")
                    on:click=|e| e.stop_propagation()
                >
                    <input
                        node_ref=input
                        type="text"
                        role="combobox"
                        aria-expanded="true"
                        aria-autocomplete="list"
                        aria-controls="palette-list"
                        aria-activedescendant=move || {
                            (!shown.with(Vec::is_empty))
                                .then(|| format!("palette-option-{}", selected.get()))
                        }
                        placeholder=t("Palast oder Befehl suchen …")
                        prop:value=move || query.get()
                        on:input=move |e| {
                            query.set(event_target_value(&e));
                            selected.set(0);
                        }
                    />
                    <ul role="listbox" id="palette-list" aria-label=t("Treffer")>
                        {move || {
                            shown
                                .get()
                                .into_iter()
                                .enumerate()
                                .map(|(i, command)| {
                                    let pick = command.clone();
                                    view! {
                                        <li
                                            id=format!("palette-option-{i}")
                                            role="option"
                                            class:on=move || selected.get() == i
                                            aria-selected=move || (selected.get() == i).to_string()
                                            on:mouseenter=move |_| selected.set(i)
                                            on:click=move |_| run(pick.clone())
                                        >
                                            <span>{command.label.clone()}</span>
                                            <small>{command.hint}</small>
                                        </li>
                                    }
                                })
                                .collect_view()
                        }}
                    </ul>
                    <p class="notes" role="status">{move || note.get()}</p>
                </div>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::{SwipeStart, is_palette_swipe, matches};

    #[test]
    fn every_word_must_match() {
        assert!(matches("Planeten üben", "plan üb"));
        assert!(matches("Planeten üben", ""));
        assert!(!matches("Planeten öffnen", "plan üb"));
    }

    #[test]
    fn swipe_requires_a_quick_vertical_pull_without_scrolling() {
        let start = SwipeStart {
            pointer: 1,
            x: 100.0,
            y: 20.0,
            time_ms: 0.0,
            scroll_y: 0.0,
        };
        assert!(is_palette_swipe(start, 120.0, 130.0, 400.0, 0.0));
        assert!(!is_palette_swipe(start, 180.0, 130.0, 400.0, 0.0));
        assert!(!is_palette_swipe(start, 120.0, 100.0, 400.0, 0.0));
        assert!(!is_palette_swipe(start, 120.0, 130.0, 950.0, 0.0));
        assert!(!is_palette_swipe(start, 120.0, 130.0, 400.0, 20.0));
    }
}
