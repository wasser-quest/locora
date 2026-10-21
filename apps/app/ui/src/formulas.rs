//! The formula editor, a modal over the room editor: the formulas bound to a locus, the workbook
//! tables of the palace (and the shared ones), and a calculator. The same as `locora table`,
//! `bind`, `unbind` and `calc` on the command line.

use crate::i18n::t;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_WORKBOOK, CMD_WORKBOOK_CALC, CMD_WORKBOOK_CHANGE, WorkbookBinding, WorkbookCell,
    WorkbookChange, WorkbookRow, WorkbookView,
};
use serde::Serialize;

use crate::tauri::call;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkbookArgs {
    palace_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeArgs {
    palace_id: String,
    change: WorkbookChange,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CalcArgs {
    palace_id: String,
    expression: String,
}

/// The locus fields a formula can set, with their labels.
const FIELDS: [(&str, &str); 6] = [
    ("place", "Ort"),
    ("item", "Inhalt"),
    ("x", "x (0–1)"),
    ("y", "y (0–1)"),
    ("visible", "Sichtbar"),
    ("active", "Aktiv"),
];

const KINDS: [&str; 8] = [
    "text", "number", "boolean", "json", "formula", "asset", "palace", "room",
];

/// Shared state of the open editor: the workbook, and whether a change is on its way.
#[derive(Clone, Copy)]
struct Book {
    palace_id: StoredValue<String>,
    view: RwSignal<Option<Result<WorkbookView, String>>>,
    busy: RwSignal<bool>,
    message: RwSignal<Option<Result<String, String>>>,
}

impl Book {
    fn apply(self, change: WorkbookChange, done: &'static str) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.message.set(None);
        let palace_id = self.palace_id.get_value();
        spawn_local(async move {
            match call::<WorkbookView>(CMD_WORKBOOK_CHANGE, ChangeArgs { palace_id, change }).await
            {
                Ok(view) => {
                    self.view.set(Some(Ok(view)));
                    self.message.set(Some(Ok(t(done).into())));
                }
                Err(error) => self.message.set(Some(Err(error))),
            }
            self.busy.set(false);
        });
    }
}

/// The modal. `locus` is the locus selected in the editor, if any; `on_close` runs when it closes,
/// so the editor reloads the palace (bound fields change what it shows).
#[component]
pub fn FormulaEditor(
    palace_id: String,
    locus: Option<String>,
    on_close: Callback<()>,
) -> impl IntoView {
    let book = Book {
        palace_id: StoredValue::new(palace_id.clone()),
        view: RwSignal::new(None),
        busy: RwSignal::new(false),
        message: RwSignal::new(None),
    };
    spawn_local(async move {
        book.view.set(Some(
            call::<WorkbookView>(CMD_WORKBOOK, WorkbookArgs { palace_id }).await,
        ));
    });
    let chosen = RwSignal::new(locus.unwrap_or_default());
    let shared = RwSignal::new(false);
    let close = move || on_close.run(());
    let keys = window_event_listener(leptos::ev::keydown, move |event| {
        if event.key() == "Escape" {
            close();
        }
    });
    on_cleanup(move || keys.remove());
    view! {
        <div class="palette-backdrop" on:click=move |_| close()>
            <div class="card formula-editor" role="dialog" aria-modal="true" aria-label=t("Formeln")
                on:click=|event| event.stop_propagation()>
                <div class="formula-head">
                    <h2>{t("Formeln")}</h2>
                    <button type="button" on:click=move |_| close()>{t("Schließen")}</button>
                </div>
                {move || match book.view.get() {
                    None => view! { <p class="progress">{t("Formeln werden geladen …")}</p> }.into_any(),
                    Some(Err(error)) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
                    Some(Ok(view)) => {
                        // Default to the first locus when none was selected in the editor.
                        if !view.loci.iter().any(|l| l.id == chosen.get_untracked()) {
                            chosen.set(view.loci.first().map(|l| l.id.clone()).unwrap_or_default());
                        }
                        let loci = view.loci.clone();
                        let rows = if shared.get() { view.shared.clone() } else { view.rows.clone() };
                        view! {
                            <section>
                                <h3>{t("Bindungen")}</h3>
                                <p class="notes">{t("Eine Formel setzt ein Feld des Orts, z. B. Zahlen.a.value + 1. Gebundene Felder sind im Editor schreibgeschützt.")}</p>
                                {if loci.is_empty() {
                                    view! { <p class="notes">{t("Dieser Palast hat noch keine Orte.")}</p> }.into_any()
                                } else {
                                    view! {
                                        <label class="input"><span>{t("Ort")}</span>
                                            <select prop:value=move || chosen.get() on:change=move |e| chosen.set(event_target_value(&e))>
                                                {loci.iter().map(|l| view! {
                                                    <option value=l.id.clone()>{format!("{}. {}", l.number, l.place)}</option>
                                                }).collect_view()}
                                            </select>
                                        </label>
                                        {move || {
                                            let id = chosen.get();
                                            let bindings = loci.iter().find(|l| l.id == id).map(|l| l.bindings.clone()).unwrap_or_default();
                                            FIELDS.iter().map(|(field, label)| {
                                                let current = bindings.iter().find(|b| b.field == *field).cloned();
                                                view! { <BindingRow book=book locus=id.clone() field=field label=label current=current /> }
                                            }).collect_view()
                                        }}
                                    }.into_any()
                                }}
                            </section>
                            <section>
                                <h3>{t("Tabellen")}</h3>
                                <label><input type="checkbox" prop:checked=move || shared.get()
                                    on:change=move |e| shared.set(event_target_checked(&e)) />
                                    {t("Gemeinsame Tabellen (für alle Paläste)")}</label>
                                {if rows.is_empty() {
                                    view! { <p class="notes">{t("Noch keine Zellen.")}</p> }.into_any()
                                } else {
                                    view! {
                                        <div class="formula-cells">
                                            {rows.into_iter().flat_map(|row| {
                                                let WorkbookRow { table, key, cells } = row;
                                                cells.into_iter().map(move |cell| {
                                                    view! { <CellRow book=book shared=shared.get_untracked() table=table.clone() key=key.clone() cell=cell /> }
                                                })
                                            }).collect_view()}
                                        </div>
                                    }.into_any()
                                }}
                                <NewCell book=book shared=shared />
                            </section>
                        }.into_any()
                    }
                }}
                <Calculator book=book />
                {move || book.message.get().map(|result| match result {
                    Ok(text) => view! { <p class="verdict ok" role="status">{text}</p> }.into_any(),
                    Err(text) => view! { <p class="notes" role="alert">{text}</p> }.into_any(),
                })}
            </div>
        </div>
    }
}

/// One field of the locus: its formula, what it gives now, and saving or removing it.
#[component]
fn BindingRow(
    book: Book,
    locus: String,
    field: &'static str,
    label: &'static str,
    current: Option<WorkbookBinding>,
) -> impl IntoView {
    let expression = RwSignal::new(
        current
            .as_ref()
            .map(|b| b.expression.clone())
            .unwrap_or_default(),
    );
    let bound = current.is_some();
    let result = current.map(|b| match (b.value, b.error) {
        (_, Some(error)) => Err(error),
        (Some(value), None) => Ok(value),
        (None, None) => Ok(String::new()),
    });
    let locus = StoredValue::new(locus);
    let save = move || {
        let expression = expression.get_untracked();
        book.apply(
            WorkbookChange::Bind {
                locus_id: locus.get_value(),
                field: field.into(),
                expression,
            },
            "Bindung gespeichert.",
        );
    };
    let remove = move || {
        book.apply(
            WorkbookChange::Unbind {
                locus_id: locus.get_value(),
                field: field.into(),
            },
            "Bindung entfernt.",
        );
    };
    view! {
        <form class="formula-binding" on:submit=move |e| { e.prevent_default(); save(); }>
            <label class="input"><span>{t(label)}</span>
                <input type="text" spellcheck="false" placeholder=t("keine Formel") bind:value=expression />
            </label>
            {result.map(|result| match result {
                Ok(value) => view! { <p class="meta">{format!("= {value}")}</p> }.into_any(),
                Err(error) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
            })}
            <div class="tabs">
                <button type="submit" disabled=move || book.busy.get() || expression.get().trim().is_empty()>{t("Speichern")}</button>
                <Show when=move || bound>
                    <button type="button" disabled=move || book.busy.get() on:click=move |_| remove()>{t("Lösen")}</button>
                </Show>
            </div>
        </form>
    }
}

/// One cell of a table: its type and value, changed in place or removed.
#[component]
fn CellRow(
    book: Book,
    shared: bool,
    table: String,
    key: String,
    cell: WorkbookCell,
) -> impl IntoView {
    let address = format!("{table}.{key}.{}", cell.column);
    let kind = RwSignal::new(cell.kind.clone());
    let value = RwSignal::new(cell.value.clone());
    let place = StoredValue::new((table, key, cell.column));
    let save = move || {
        let (table, key, column) = place.get_value();
        book.apply(
            WorkbookChange::SetCell {
                shared,
                table,
                key,
                column,
                kind: kind.get_untracked(),
                value: value.get_untracked(),
            },
            "Zelle gespeichert.",
        );
    };
    let remove = move || {
        let (table, key, column) = place.get_value();
        book.apply(
            WorkbookChange::RemoveCell {
                shared,
                table,
                key,
                column,
            },
            "Zelle gelöscht.",
        );
    };
    view! {
        <form class="formula-cell" on:submit=move |e| { e.prevent_default(); save(); }>
            <code>{address}</code>
            <select prop:value=move || kind.get() on:change=move |e| kind.set(event_target_value(&e))>
                {KINDS.iter().map(|k| view! { <option value=*k>{*k}</option> }).collect_view()}
            </select>
            <input type="text" spellcheck="false" bind:value=value />
            <button type="submit" disabled=move || book.busy.get()>{t("Speichern")}</button>
            <button type="button" disabled=move || book.busy.get() on:click=move |_| remove()>{t("Löschen")}</button>
        </form>
    }
}

/// A new cell: table, key, column, type and value.
#[component]
fn NewCell(book: Book, shared: RwSignal<bool>) -> impl IntoView {
    let table = RwSignal::new(String::new());
    let key = RwSignal::new(String::new());
    let column = RwSignal::new(String::from("value"));
    let kind = RwSignal::new(String::from("number"));
    let value = RwSignal::new(String::new());
    let add = move || {
        book.apply(
            WorkbookChange::SetCell {
                shared: shared.get_untracked(),
                table: table.get_untracked().trim().into(),
                key: key.get_untracked().trim().into(),
                column: column.get_untracked().trim().into(),
                kind: kind.get_untracked(),
                value: value.get_untracked(),
            },
            "Zelle gespeichert.",
        );
        value.set(String::new());
    };
    let incomplete = move || {
        [table, key, column]
            .iter()
            .any(|s| s.get().trim().is_empty())
    };
    view! {
        <form class="formula-new" on:submit=move |e| { e.prevent_default(); add(); }>
            <label class="input"><span>{t("Tabelle")}</span><input type="text" spellcheck="false" bind:value=table /></label>
            <label class="input"><span>{t("Schlüssel")}</span><input type="text" spellcheck="false" bind:value=key /></label>
            <label class="input"><span>{t("Spalte")}</span><input type="text" spellcheck="false" bind:value=column /></label>
            <label class="input"><span>{t("Typ")}</span>
                <select prop:value=move || kind.get() on:change=move |e| kind.set(event_target_value(&e))>
                    {KINDS.iter().map(|k| view! { <option value=*k>{*k}</option> }).collect_view()}
                </select>
            </label>
            <label class="input"><span>{t("Wert")}</span><input type="text" spellcheck="false" bind:value=value /></label>
            <button type="submit" class="primary" disabled=move || book.busy.get() || incomplete()>{t("Zelle hinzufügen")}</button>
        </form>
    }
}

/// Tries a formula with the palace's tables; nothing is saved.
#[component]
fn Calculator(book: Book) -> impl IntoView {
    let expression = RwSignal::new(String::new());
    let result = RwSignal::new(None::<Result<String, String>>);
    let run = move || {
        let expression = expression.get_untracked();
        let palace_id = book.palace_id.get_value();
        spawn_local(async move {
            result.set(Some(
                call::<String>(
                    CMD_WORKBOOK_CALC,
                    CalcArgs {
                        palace_id,
                        expression,
                    },
                )
                .await,
            ));
        });
    };
    view! {
        <section>
            <h3>{t("Rechner")}</h3>
            <form class="formula-calc" on:submit=move |e| { e.prevent_default(); run(); }>
                <label class="input"><span>{t("Formel")}</span><input type="text" spellcheck="false" bind:value=expression /></label>
                <button type="submit" disabled=move || expression.get().trim().is_empty()>{t("Berechnen")}</button>
            </form>
            {move || result.get().map(|result| match result {
                Ok(value) => view! { <p class="verdict ok" role="status">{format!("= {value}")}</p> }.into_any(),
                Err(error) => view! { <p class="notes" role="alert">{error}</p> }.into_any(),
            })}
        </section>
    }
}
