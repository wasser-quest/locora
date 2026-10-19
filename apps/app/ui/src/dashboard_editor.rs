//! The page editor: the blocks of the page on screen, the pages, the cards plugins suggest, the
//! look, and copying the layout to text and back.

use leptos::prelude::*;

use crate::dashboard::{
    Catalog, Entry, Layout, Page, Pages, Placement, START, Size, block_id, ensure_start, known,
    label, page_label, resolve, save,
};
use crate::i18n::{plugin_text, t, tf};
use crate::look::{self, Look};

fn placement_value(placement: Placement) -> &'static str {
    match placement {
        Placement::Shown => "shown",
        Placement::Folded => "folded",
        Placement::Hidden => "hidden",
    }
}

fn placement_from(value: &str) -> Option<Placement> {
    match value {
        "shown" => Some(Placement::Shown),
        "folded" => Some(Placement::Folded),
        "hidden" => Some(Placement::Hidden),
        _ => None,
    }
}

fn size_value(size: Size) -> &'static str {
    match size {
        Size::Full => "full",
        Size::Compact => "compact",
    }
}

fn size_from(value: &str) -> Option<Size> {
    match value {
        "full" => Some(Size::Full),
        "compact" => Some(Size::Compact),
        _ => None,
    }
}

/// Changes the blocks of a page as they are shown (resolved first), then keeps them.
fn rearrange(page: &mut Page, catalog: &Catalog, edit: impl FnOnce(&mut Vec<Entry>)) {
    let mut entries = resolve(&page.entries, &known(catalog), page.id == START);
    edit(&mut entries);
    page.entries = entries;
}

/// The page editor. Every change goes to the page on screen and is saved at once.
#[component]
pub fn Editor(catalog: Catalog, open: RwSignal<bool>) -> impl IntoView {
    let layout = expect_context::<Layout>().0;
    let catalog = StoredValue::new(catalog);
    let change = move |edit: &dyn Fn(&mut Page, &Catalog)| {
        layout.update(|pages| {
            catalog.with_value(|catalog| edit(pages.current_mut(), catalog));
            save(&pages.list);
        });
    };
    let entries = move || {
        let pages = layout.get();
        let page = pages.current();
        catalog.with_value(|catalog| resolve(&page.entries, &known(catalog), page.id == START))
    };
    let new_name = RwSignal::new(String::new());
    let is_start = move || layout.get().current().id == START;

    let create = move |_| {
        let name: String = new_name.get_untracked().trim().chars().take(40).collect();
        if name.is_empty() {
            return;
        }
        let id = format!("page-{}", js_sys::Date::now() as u64);
        layout.update(|pages| {
            pages.list.push(Page {
                id: id.clone(),
                name,
                entries: Vec::new(),
            });
            pages.current = id;
            save(&pages.list);
        });
        new_name.set(String::new());
    };

    // A page a plugin suggests: its declared blocks, shown in its order.
    let add_suggested = move |plugin: String, blocks: Vec<String>, name: String| {
        let entries: Vec<Entry> = catalog.with_value(|catalog| {
            blocks
                .iter()
                .map(|block| block_id(&plugin, block))
                .filter(|id| {
                    catalog
                        .blocks
                        .iter()
                        .any(|declared| block_id(&declared.plugin, &declared.id) == *id)
                })
                .map(|id| Entry {
                    id,
                    placement: Placement::Shown,
                    size: Size::Full,
                })
                .collect()
        });
        let id = format!("page-{}", js_sys::Date::now() as u64);
        layout.update(|pages| {
            pages.list.push(Page {
                id: id.clone(),
                name,
                entries,
            });
            pages.current = id;
            save(&pages.list);
        });
    };

    let transfer = RwSignal::new(String::new());
    let transfer_error = RwSignal::new(String::new());
    let export = move |_| {
        let pages = layout.get_untracked();
        transfer.set(serde_json::to_string_pretty(&pages.list).unwrap_or_default());
        transfer_error.set(String::new());
    };
    let import = move |_| match serde_json::from_str::<Vec<Page>>(&transfer.get_untracked()) {
        Ok(mut list) => {
            ensure_start(&mut list);
            save(&list);
            layout.set(Pages {
                list,
                current: START.to_owned(),
            });
            transfer_error.set(String::new());
        }
        Err(error) => transfer_error.set(tf("Das Layout ist ungültig: {0}", &[&error])),
    };

    let look = RwSignal::new(look::load());
    let set_look = move |change: fn(&mut Look, String), value: String| {
        look.update(|look| change(look, value));
        look::save(&look.get_untracked());
    };

    view! {
        <section class="card panel dashboard" aria-label=t("Seiten anpassen")>
            <h2>{t("Seiten anpassen")}</h2>
            <p class="dashboard-note">
                {t("Was jede Seite zeigt und wo. „Im Plugins-Bereich“ klappt unter „Plugins“ zu. Eigene Seiten zeigen nur, was du dort einfügst.")}
            </p>
            <div class="row">
                <label>
                    {t("Seite")}
                    <select on:change=move |event| {
                        let id = event_target_value(&event);
                        layout.update(|pages| pages.current = id);
                    }>
                        {move || {
                            let pages = layout.get();
                            pages
                                .list
                                .iter()
                                .map(|page| {
                                    let selected = page.id == pages.current;
                                    view! {
                                        <option value=page.id.clone() selected=selected>
                                            {page_label(page)}
                                        </option>
                                    }
                                })
                                .collect_view()
                        }}
                    </select>
                </label>
            </div>
            {move || (!is_start()).then(|| {
                view! {
                    <div class="row">
                        <input
                            aria-label=t("Seitenname")
                            prop:value=move || layout.get().current().name.clone()
                            on:change=move |event| {
                                let name: String = event_target_value(&event).trim().chars().take(40).collect();
                                if !name.is_empty() {
                                    change(&move |page, _| page.name = name.clone());
                                }
                            }
                        />
                        <button on:click=move |_| {
                            layout.update(|pages| {
                                let current = pages.current.clone();
                                pages.list.retain(|page| page.id != current);
                                pages.current = START.to_owned();
                                save(&pages.list);
                            });
                        }>{t("Seite löschen")}</button>
                    </div>
                }
            })}
            <ol class="dashboard-list">
                {move || {
                    let entries = entries();
                    let last = entries.len().saturating_sub(1);
                    catalog.with_value(|catalog| {
                        entries
                            .into_iter()
                            .enumerate()
                            .map(|(index, entry)| {
                                let id = entry.id.clone();
                                let name = label(&id, catalog);
                                let placement = entry.placement;
                                let size = entry.size;
                                let set_placement = {
                                    let id = id.clone();
                                    move |event: leptos::ev::Event| {
                                        let Some(placement) = placement_from(&event_target_value(&event))
                                        else {
                                            return;
                                        };
                                        change(&|page, catalog| {
                                            rearrange(page, catalog, |entries| {
                                                if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                                                    entry.placement = placement;
                                                }
                                            })
                                        });
                                    }
                                };
                                let set_size = {
                                    let id = id.clone();
                                    move |event: leptos::ev::Event| {
                                        let Some(size) = size_from(&event_target_value(&event)) else {
                                            return;
                                        };
                                        change(&|page, catalog| {
                                            rearrange(page, catalog, |entries| {
                                                if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                                                    entry.size = size;
                                                }
                                            })
                                        });
                                    }
                                };
                                let swap = move |from: usize, to: usize| {
                                    change(&|page, catalog| rearrange(page, catalog, |entries| entries.swap(from, to)));
                                };
                                view! {
                                    <li class="dashboard-row">
                                        <span class="dashboard-name">{name.clone()}</span>
                                        <select aria-label=name.clone() on:change=set_placement>
                                            <option value=placement_value(Placement::Shown) selected=placement == Placement::Shown>
                                                {t("Zeigen")}
                                            </option>
                                            <option value=placement_value(Placement::Folded) selected=placement == Placement::Folded>
                                                {t("Im Plugins-Bereich")}
                                            </option>
                                            <option value=placement_value(Placement::Hidden) selected=placement == Placement::Hidden>
                                                {t("Nicht auf dieser Seite")}
                                            </option>
                                        </select>
                                        <select aria-label=tf("Größe von {0}", &[&name]) on:change=set_size>
                                            <option value=size_value(Size::Full) selected=size == Size::Full>{t("Ganz")}</option>
                                            <option value=size_value(Size::Compact) selected=size == Size::Compact>{t("Kompakt")}</option>
                                        </select>
                                        <button disabled=index == 0 aria-label=t("Nach oben") on:click=move |_| {
                                            if index > 0 {
                                                swap(index, index - 1);
                                            }
                                        }>"↑"</button>
                                        <button disabled=index == last aria-label=t("Nach unten") on:click=move |_| {
                                            if index < last {
                                                swap(index, index + 1);
                                            }
                                        }>"↓"</button>
                                    </li>
                                }
                            })
                            .collect_view()
                    })
                }}
            </ol>

            {move || {
                let suggestions = catalog.with_value(|catalog| catalog.suggestions.clone());
                (!suggestions.is_empty()).then(|| {
                    let buttons = suggestions
                        .into_iter()
                        .map(|suggestion| {
                            let name = plugin_text(&suggestion.name);
                            let label_text = tf("Seite „{0}“ hinzufügen", &[&name]);
                            let plugin = suggestion.plugin.clone();
                            let blocks = suggestion.blocks.clone();
                            view! {
                                <button on:click=move |_| add_suggested(plugin.clone(), blocks.clone(), name.clone())>
                                    {label_text}
                                </button>
                            }
                        })
                        .collect_view();
                    view! {
                        <div class="dashboard-suggestions">
                            <h3>{t("Vorschläge von Plugins")}</h3>
                            <div class="row">{buttons}</div>
                        </div>
                    }
                })
            }}

            <h3>{t("Aussehen")}</h3>
            <div class="row">
                <label>
                    {t("Akzentfarbe")}
                    <select on:change=move |event| set_look(|look, value| look.accent = value, event_target_value(&event))>
                        {look::ACCENTS
                            .iter()
                            .map(|(id, name, _)| {
                                let selected = look.get().accent == *id;
                                view! { <option value=*id selected=selected>{t(*name)}</option> }
                            })
                            .collect_view()}
                    </select>
                </label>
                <label>
                    {t("Dichte")}
                    <select on:change=move |event| set_look(|look, value| look.density = value, event_target_value(&event))>
                        <option value="normal" selected=look.get().density == "normal">{t("Normal")}</option>
                        <option value="compact" selected=look.get().density == "compact">{t("Kompakt")}</option>
                    </select>
                </label>
            </div>

            <h3>{t("Layout kopieren")}</h3>
            <p class="dashboard-note">
                {t("Exportiert alle Seiten als Text. Den Text kannst du auf einem anderen Gerät wieder einfügen.")}
            </p>
            <textarea
                class="layout-text"
                rows="6"
                aria-label=t("Layout als Text")
                prop:value=move || transfer.get()
                on:input=move |event| transfer.set(event_target_value(&event))
            ></textarea>
            <div class="row">
                <button on:click=export>{t("Exportieren")}</button>
                <button on:click=import>{t("Übernehmen")}</button>
            </div>
            {move || (!transfer_error.get().is_empty()).then(|| view! {
                <p class="notes" role="alert">{transfer_error.get()}</p>
            })}

            <div class="row">
                {move || is_start().then(|| view! {
                    <button on:click=move |_| change(&|page, _| page.entries.clear())>{t("Zurücksetzen")}</button>
                })}
                <input
                    aria-label=t("Seitenname")
                    placeholder=t("Neue Seite")
                    prop:value=move || new_name.get()
                    on:input=move |event| new_name.set(event_target_value(&event))
                />
                <button on:click=create>{t("Neue Seite")}</button>
                <button class="primary" on:click=move |_| open.set(false)>{t("Fertig")}</button>
            </div>
        </section>
    }
}
