//! Pages of the app's home: the start page and extra pages that show only the blocks chosen for
//! them. A block is a built-in section, one plugin's tile, or a card a plugin declared. Kept per
//! viewer in the browser; the editor lives in dashboard_editor.rs.

use leptos::prelude::*;
use locora_app_api::{PluginBlock, PluginPage, TileView};
use serde::{Deserialize, Serialize};

use crate::i18n::{plugin_text, t};
use crate::Tiles;

pub const KEY: &str = "locora-dashboard-v2";

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

/// How much of a block is shown: the whole card, or only its heading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    #[default]
    Full,
    Compact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub placement: Placement,
    #[serde(default)]
    pub size: Size,
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

    pub fn current_mut(&mut self) -> &mut Page {
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

/// Everything a page can show from outside the built-in sections: plugin tiles, the cards
/// plugins declared, and the pages they suggest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Catalog {
    pub tiles: Vec<TileView>,
    pub blocks: Vec<PluginBlock>,
    pub suggestions: Vec<PluginPage>,
}

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

/// The id of a card a plugin declared.
pub fn block_id(plugin: &str, id: &str) -> String {
    format!("block:{plugin}/{id}")
}

/// Every block the catalog offers, with its default placement on the start page.
pub fn known(catalog: &Catalog) -> Vec<(String, Placement)> {
    let mut known: Vec<(String, Placement)> = BUILT_IN
        .iter()
        .map(|(id, placement)| ((*id).to_owned(), *placement))
        .collect();
    known.extend(
        catalog
            .tiles
            .iter()
            .map(|tile| (tile_id(&tile.plugin), Placement::Folded)),
    );
    known.extend(
        catalog
            .blocks
            .iter()
            .map(|block| (block_id(&block.plugin, &block.id), Placement::Folded)),
    );
    known
}

pub fn load() -> Pages {
    let mut list = web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(KEY).ok().flatten())
        .and_then(|value| serde_json::from_str::<Vec<Page>>(&value).ok())
        .unwrap_or_default();
    ensure_start(&mut list);
    Pages {
        list,
        current: START.to_owned(),
    }
}

/// The start page must exist; it goes first.
pub fn ensure_start(list: &mut Vec<Page>) {
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
}

pub fn save(pages: &[Page]) {
    let storage = web_sys::window().and_then(|window| window.local_storage().ok().flatten());
    if let (Some(storage), Ok(json)) = (storage, serde_json::to_string(pages)) {
        let _ = storage.set_item(KEY, &json);
    }
}

/// The blocks of a page in order, with their placement. Saved entries keep their place; a block
/// missing from the page gets its default on the start page and is hidden on extra pages.
/// Blocks that no longer exist are dropped; a block installed since shows up as new.
pub fn resolve(saved: &[Entry], known: &[(String, Placement)], start: bool) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for entry in saved {
        let is_known = known.iter().any(|(id, _)| *id == entry.id);
        if is_known && !out.iter().any(|kept| kept.id == entry.id) {
            out.push(entry.clone());
        }
    }
    for (id, default) in known {
        if !out.iter().any(|entry| entry.id == *id) {
            let placement = if start { *default } else { Placement::Hidden };
            out.push(Entry {
                id: id.clone(),
                placement,
                size: Size::Full,
            });
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
pub fn label(id: &str, catalog: &Catalog) -> String {
    match id {
        "due" => t("Heute fällig").to_owned(),
        "palaces" => t("Paläste").to_owned(),
        "tutorial" => t("Locora lernen").to_owned(),
        "formulas" => t("Formeln verstehen").to_owned(),
        "achievements" => t("Erfolgs-Medaillen").to_owned(),
        "activity" => t("Aktivität").to_owned(),
        _ => catalog
            .tiles
            .iter()
            .find(|tile| tile_id(&tile.plugin) == id)
            .map(|tile| plugin_text(&tile.title))
            .or_else(|| {
                catalog
                    .blocks
                    .iter()
                    .find(|block| block_id(&block.plugin, &block.id) == id)
                    .map(|block| plugin_text(&block.title))
            })
            .unwrap_or_else(|| id.to_owned()),
    }
}

/// The view of a plugin's tile or declared card; `None` for built-in or unknown ids.
pub fn plugin_view(catalog: &Catalog, id: &str) -> Option<AnyView> {
    if let Some(tile) = catalog.tiles.iter().find(|tile| tile_id(&tile.plugin) == id) {
        let tile = tile.clone();
        return Some(view! { <Tiles tiles=vec![tile]/> }.into_any());
    }
    let block = catalog
        .blocks
        .iter()
        .find(|block| block_id(&block.plugin, &block.id) == id)?;
    let title = plugin_text(&block.title);
    let text = plugin_text(&block.text);
    Some(
        view! {
            <section class="card panel plugin-block">
                <h2>{title}</h2>
                <p class="meta">{text}</p>
            </section>
        }
        .into_any(),
    )
}

/// Wraps a block so its size can be set in CSS.
pub fn sized(view: AnyView, size: Size) -> AnyView {
    view! {
        <div class="block" class:compact=size == Size::Compact>
            {view}
        </div>
    }
    .into_any()
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
