//! Command palette (Ctrl/⌘+K): type to find a palace or a command, arrows
//! to choose, Enter to run, Escape to close.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{CMD_HOME, Home, JourneyStart};

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
            label: "Weiter – fällige Loci wiederholen".into(),
            hint: "Journey",
            target: Screen::Journey(JourneyStart {
                due_only: true,
                ..Default::default()
            }),
        },
        Command {
            label: "Übersicht".into(),
            hint: "Start",
            target: Screen::Home,
        },
        Command {
            label: "Einstellungen & Statistik".into(),
            hint: "⚙",
            target: Screen::Settings,
        },
    ];
    for p in &home.palaces {
        list.push(Command {
            label: format!("{} öffnen", p.name),
            hint: "Palast",
            target: Screen::Palace(p.id.clone()),
        });
        list.push(Command {
            label: format!("{} üben", p.name),
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
            note.set("Erst die Änderungen im Editor speichern oder verwerfen.".into());
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
    on_cleanup(move || keys.remove());

    view! {
        <Show when=move || open.get()>
            <div class="palette-backdrop" on:click=move |_| close()>
                <div
                    class="palette card"
                    role="dialog"
                    aria-modal="true"
                    aria-label="Befehlspalette"
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
                        placeholder="Palast oder Befehl suchen …"
                        prop:value=move || query.get()
                        on:input=move |e| {
                            query.set(event_target_value(&e));
                            selected.set(0);
                        }
                    />
                    <ul role="listbox" id="palette-list" aria-label="Treffer">
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
    use super::matches;

    #[test]
    fn every_word_must_match() {
        assert!(matches("Planeten üben", "plan üb"));
        assert!(matches("Planeten üben", ""));
        assert!(!matches("Planeten öffnen", "plan üb"));
    }
}
