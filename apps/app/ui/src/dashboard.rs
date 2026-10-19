//! Pages of the app's home: the start page and extra pages that show only the blocks chosen for
//! them. A block is a built-in section or one plugin's tile. Kept per viewer in the browser.

use leptos::prelude::*;
use locora_app_api::TileView;
use serde::{Deserialize, Serialize};

use crate::i18n::{plugin_text, t};

const KEY: &str = "locora-dashboard-v2";

/// The start page's id. It always exists and cannot be deleted.
pub const START: &str = "start";

/// Where a block sits on its page: shown, folded under "Plugins", or not on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    Shown,
    Folded,
    Hidden,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub placement: Placement,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    /// The name of an extra page; the start page is named by the app.
    #[serde(default)]
    pub name: String,
    /// The blocks in order. Empty until the page is first arranged.
    #[serde(default)]
    pub entries: Vec<Entry>,
}

/// All pages and the one on screen. The start page opens first; the choice is not saved.
#[derive(Debug, Clone, PartialEq)]
pub struct Pages {
    pub list: Vec<Page>,
    pub current: String,
}

impl Pages {
    fn index(&self) -> usize {
        self.list.iter().position(|page| page.id == self.current).unwrap_or(0)
    }

    pub fn current(&self) -> &Page {
        &self.list[self.index()]
    }

    fn current_mut(&mut self) -> &mut Page {
        let index = self.index();
        &mut self.list[index]
    }
}

/// The pages, shared by the home screen and its editor.
#[derive(Debug, Clone, Copy)]
pub struct Layout(pub RwSignal<Pages>);

/// Whether the page editor is open.
#[derive(Debug, Clone, Copy)]
pub struct Open(pub RwSignal<bool>);

/// The built-in blocks with their placement on the start page. Palaces come right after the
/// due card; the plugin blocks wait behind the "Plugins" fold.
const BUILT_IN: &[(&str, Placement)] = &[
    ("due", Placement::Shown),
    ("palaces", Placement::Shown),
    ("tutorial", Placement::Shown),
    ("formulas", Placement::Shown),
    ("achievements", Placement::Folded),
    ("activity", Placement::Folded),
];

/// The id of a plugin's tile block.
pub fn tile_id(plugin: &str) -> String {
    format!("tile:{plugin}")
}

pub fn load() -> Pages {
    let mut list = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<Vec<Page>>(&value).ok())
        .unwrap_or_default();
    if !list.iter().any(|page| page.id == START) {
        list.insert(
            0,
            Page {
                id: START.to_owned(),
                name: String::new(),
                entries: Vec::new(),
            },
        );
    }
    Pages {
        list,
        current: START.to_owned(),
    }
}

pub fn save(pages: &[Page]) {
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
    if let (Some(storage), Ok(json)) = (storage, serde_json::to_string(pages)) {
        let _ = storage.set_item(KEY, &json);
    }
}

/// The blocks of a page in order, with their placement. Saved entries keep their place; a block
/// missing from the page gets its default on the start page and is hidden on extra pages.
/// Blocks that no longer exist are dropped; a plugin installed since shows up as a new block.
pub fn resolve(saved: &[Entry], tiles: &[TileView], start: bool) -> Vec<Entry> {
    let mut known: Vec<(String, Placement)> = BUILT_IN
        .iter()
        .map(|(id, placement)| ((*id).to_owned(), *placement))
        .collect();
    known.extend(tiles.iter().map(|tile| (tile_id(&tile.plugin), Placement::Folded)));
    let mut out: Vec<Entry> = Vec::new();
    for entry in saved {
        let is_known = known.iter().any(|(id, _)| *id == entry.id);
        if is_known && !out.iter().any(|kept| kept.id == entry.id) {
            out.push(entry.clone());
        }
    }
    for (id, default) in known {
        if !out.iter().any(|entry| entry.id == id) {
            let placement = if start { default } else { Placement::Hidden };
            out.push(Entry { id, placement });
        }
    }
    out
}

/// The name of a page in the tab bar.
pub fn page_label(page: &Page) -> String {
    if page.id == START {
        t("Start").to_owned()
    } else {
        page.name.clone()
    }
}

/// The name of a block in the editor.
pub fn label(id: &str, tiles: &[TileView]) -> String {
    match id {
        "due" => t("Heute fällig").to_owned(),
        "palaces" => t("Paläste").to_owned(),
        "tutorial" => t("Locora lernen").to_owned(),
        "formulas" => t("Formeln verstehen").to_owned(),
        "achievements" => t("Erfolge").to_owned(),
        "activity" => t("Aktivität").to_owned(),
        _ => tiles
            .iter()
            .find(|tile| tile_id(&tile.plugin) == id)
            .map(|tile| plugin_text(&tile.title))
            .unwrap_or_else(|| id.to_owned()),
    }
}

/// The tab bar of the pages. Only shown once there is more than the start page.
#[component]
pub fn Tabs() -> impl IntoView {
    let layout = expect_context::<Layout>().0;
    move || {
        let pages = layout.get();
        (pages.list.len() > 1).then(|| {
            let tabs = pages
                .list
                .iter()
                .map(|page| {
                    let id = page.id.clone();
                    let active = id == pages.current;
                    view! {
                        <button
                            class:active=active
                            aria-current=active.then_some("page")
                            on:click=move |_| layout.update(|pages| pages.current = id.clone())
                        >
                            {page_label(page)}
                        </button>
                    }
                })
                .collect_view();
            view! { <nav class="pages" aria-label=t("Seiten")>{tabs}</nav> }
        })
    }
}

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

/// Changes the blocks of a page as they are shown (resolved first), then keeps them.
fn rearrange(page: &mut Page, tiles: &[TileView], edit: impl FnOnce(&mut Vec<Entry>)) {
    let mut entries = resolve(&page.entries, tiles, page.id == START);
    edit(&mut entries);
    page.entries = entries;
}

/// The editor: the blocks of the page on screen, and the pages themselves.
#[component]
pub fn Editor(tiles: Vec<TileView>, open: RwSignal<bool>) -> impl IntoView {
    let layout = expect_context::<Layout>().0;
    let tiles = StoredValue::new(tiles);
    // Every change goes to the page on screen and is saved at once.
    let change = move |edit: &dyn Fn(&mut Page, &[TileView])| {
        layout.update(|pages| {
            tiles.with_value(|tiles| edit(pages.current_mut(), tiles));
            save(&pages.list);
        });
    };
    let entries = move || {
        let pages = layout.get();
        let page = pages.current();
        tiles.with_value(|tiles| resolve(&page.entries, tiles, page.id == START))
    };
    let new_name = RwSignal::new(String::new());
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
    let is_start = move || layout.get().current().id == START;
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
                    entries
                        .into_iter()
                        .enumerate()
                        .map(|(index, entry)| {
                            let id = entry.id.clone();
                            let name = tiles.with_value(|tiles| label(&id, tiles));
                            let placement = entry.placement;
                            let set_placement = {
                                let id = id.clone();
                                move |event: leptos::ev::Event| {
                                    let Some(placement) = placement_from(&event_target_value(&event))
                                    else {
                                        return;
                                    };
                                    change(&|page, tiles| {
                                        rearrange(page, tiles, |entries| {
                                            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) {
                                                entry.placement = placement;
                                            }
                                        })
                                    });
                                }
                            };
                            let swap = move |from: usize, to: usize| {
                                change(&|page, tiles| rearrange(page, tiles, |entries| entries.swap(from, to)));
                            };
                            view! {
                                <li class="dashboard-row">
                                    <span class="dashboard-name">{name.clone()}</span>
                                    <select aria-label=name on:change=set_placement>
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
                }}
            </ol>
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
