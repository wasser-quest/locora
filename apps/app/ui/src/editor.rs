//! Room editor and pointer gestures; mutations and validation go through IPC.
use leptos::{html, prelude::*, task::spawn_local};
use locora_app_api::{
    CMD_EDITOR_CHANGE, EditorAction, EditorAnswer, EditorAnswerKind, EditorChange, EditorUpdated,
    LocusContent, LocusView, PalaceView, QuizOption,
};
use serde::{Deserialize, Serialize};
use web_sys::PointerEvent;

use crate::{
    Answer,
    sply::{Shown, Sply, TimedCommand, VideoBackdrop, fit_stage},
    tauri::call,
};

/// Normalize viewport coordinates against the whole stage, not a child target.
fn stage_point(x: f64, y: f64, left: f64, top: f64, width: f64, height: f64) -> Option<(f64, f64)> {
    if ![x, y, left, top, width, height]
        .iter()
        .all(|v| v.is_finite())
        || width <= 0.0
        || height <= 0.0
    {
        return None;
    }
    Some((
        ((x - left) / width).clamp(0.0, 1.0),
        ((y - top) / height).clamp(0.0, 1.0),
    ))
}

fn parse_list(text: &str) -> Result<Vec<String>, String> {
    if text.len() > locora_app_api::MAX_PASTE_BYTES {
        return Err("Die Liste darf höchstens 64 KiB groß sein".into());
    }
    let items = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    locora_app_api::validate_paste_items(&items).map_err(str::to_owned)?;
    Ok(items)
}

fn synchronized_size(draft: &str, previous: f64, committed: f64) -> Option<String> {
    (draft.parse::<f64>().ok() == Some(previous)).then(|| committed.to_string())
}

fn resized(size: f64, delta: f64, width: f64) -> f64 {
    (size + 2.0 * delta / width).clamp(0.05, 1.0)
}

fn parse_answer(kind: &str, accept: &str, quiz: &str) -> Result<EditorAnswer, String> {
    let mut answer = EditorAnswer::default();
    match kind {
        "guess" => {
            answer.kind = EditorAnswerKind::Guess;
            answer.accept = accept
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_owned())
                .collect();
        }
        "quiz" => {
            answer.kind = EditorAnswerKind::Quiz;
            answer.options = quiz
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|line| {
                    let line = line.trim();
                    let (correct, text) = if let Some(text) = line.strip_prefix('+') {
                        (true, text)
                    } else if let Some(text) = line.strip_prefix('-') {
                        (false, text)
                    } else {
                        return Err(
                            "Quiz: Jede Zeile muss mit + (richtig) oder - (falsch) beginnen"
                                .to_owned(),
                        );
                    };
                    Ok(QuizOption {
                        text: text.trim().into(),
                        correct,
                    })
                })
                .collect::<Result<_, String>>()?;
        }
        _ => {}
    }
    // This only parses the editor notation; the store validates the answer.
    Ok(answer)
}

/// Raw text is retained even when a number or quiz is temporarily incomplete.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Draft {
    place: String,
    item: String,
    note: String,
    image: String,
    size: String,
    zoom: String,
    kind: String,
    accept: String,
    quiz: String,
}

impl Draft {
    fn from_locus(l: &LocusView) -> Self {
        Self {
            place: l.place.clone(),
            item: l.item.clone(),
            note: l.note.clone(),
            image: l.image.clone().unwrap_or_default(),
            size: l.image_size.to_string(),
            zoom: l.zoom.to_string(),
            kind: match l.answer.kind {
                EditorAnswerKind::Recall => "recall",
                EditorAnswerKind::Guess => "guess",
                EditorAnswerKind::Quiz => "quiz",
            }
            .into(),
            accept: l.answer.accept.join("\n"),
            quiz: l
                .answer
                .options
                .iter()
                .map(|o| format!("{} {}", if o.correct { "+" } else { "-" }, o.text))
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn content(&self) -> Result<LocusContent, String> {
        if self.place.trim().is_empty() {
            return Err("Ort darf nicht leer sein".into());
        }
        let image_size = self
            .size
            .parse::<f64>()
            .map_err(|_| "Bildgröße muss eine Zahl sein")?;
        if !image_size.is_finite() || !(0.05..=1.0).contains(&image_size) {
            return Err("Bildgröße muss zwischen 0.05 und 1 liegen".into());
        }
        let zoom = self
            .zoom
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .map_err(|_| "Zoom muss eine Zahl sein")?;
        if !zoom.is_finite() || !(1.0..=4.0).contains(&zoom) {
            return Err("Zoom muss zwischen 1 (ganzer Raum) und 4 liegen".into());
        }
        let answer = parse_answer(&self.kind, &self.accept, &self.quiz)?;
        answer.validate().map_err(str::to_owned)?;
        Ok(LocusContent {
            place: self.place.clone(),
            item: self.item.clone(),
            note: self.note.clone(),
            image: (!self.image.trim().is_empty()).then(|| self.image.trim().to_owned()),
            image_size,
            zoom,
            answer,
        })
    }
}

/// Patches use inverse actions; structural edits use backend-issued receipts.
/// Single-use receipts rotate only after a successful acknowledgement.
#[derive(Clone, Default)]
struct History {
    undo: Vec<(EditorAction, EditorAction)>,
    redo: Vec<(EditorAction, EditorAction)>,
}

impl History {
    fn record(&mut self, before: EditorAction, after: EditorAction) {
        if before == after {
            return;
        }
        self.redo.clear();
        self.undo.push((before, after));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    fn record_receipt(&mut self, receipt: String) {
        let action = EditorAction::Restore { receipt };
        self.redo.clear();
        self.undo.push((action.clone(), action));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
    }

    fn completed(&mut self, redo: bool) {
        let (source, target) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        if let Some(entry) = source.pop() {
            target.push(entry);
        }
    }
}

fn inverse(action: &EditorAction, room: &locora_app_api::RoomView) -> Option<EditorAction> {
    let id = match action {
        EditorAction::Save { id, .. }
        | EditorAction::Move { id, .. }
        | EditorAction::Resize { id, .. } => id,
        _ => return None,
    };
    let locus = room.loci.iter().find(|l| &l.id == id)?;
    Some(match action {
        EditorAction::Save { .. } => EditorAction::Save {
            id: id.clone(),
            content: Draft::from_locus(locus).content().ok()?,
        },
        EditorAction::Move { .. } => EditorAction::Move {
            id: id.clone(),
            x: locus.x,
            y: locus.y,
        },
        EditorAction::Resize { .. } => EditorAction::Resize {
            id: id.clone(),
            image_size: locus.image_size,
        },
        _ => return None,
    })
}

/// Length of one frame for `,` and `.` in the video editor (30 fps).
const VIDEO_FRAME_SECONDS: f64 = 1.0 / 30.0;

/// Time one frame before or after `current`, never below 0 s or past a known duration.
fn frame_step_time(current: f64, forward: bool, duration: Option<f64>) -> f64 {
    let next = if forward {
        current + VIDEO_FRAME_SECONDS
    } else {
        current - VIDEO_FRAME_SECONDS
    }
    .max(0.0);
    match duration.filter(|d| d.is_finite() && *d > 0.0) {
        Some(duration) => next.min(duration),
        None => next,
    }
}

fn recoverable_draft(stored: Option<Draft>, committed: &Draft) -> Option<Draft> {
    stored.filter(|draft| draft != committed)
}

fn draft_key(palace: &str, room: &str, locus: &str) -> String {
    // Length prefixes avoid collisions even if imported IDs contain separators.
    format!(
        "locora.editor.draft.v1:{}:{palace}{}:{room}{}:{locus}",
        palace.len(),
        room.len(),
        locus.len()
    )
}

/// Access Storage through JS so this isolated editor change needs no extra
/// web-sys features. All access can fail (private mode, quota, disabled storage).
#[cfg(target_arch = "wasm32")]
fn storage(method: &str, key: &str, value: Option<&str>) -> Result<wasm_bindgen::JsValue, String> {
    use wasm_bindgen::{JsCast, JsValue};
    let error =
        |_| "Lokaler Entwurf konnte nicht gespeichert/gelesen werden (Browserspeicher)".to_owned();
    let store = js_sys::Reflect::get(
        &web_sys::window()
            .ok_or_else(|| "Kein Fenster".to_owned())?
            .into(),
        &JsValue::from_str("localStorage"),
    )
    .map_err(error)?;
    let function = js_sys::Reflect::get(&store, &JsValue::from_str(method))
        .map_err(error)?
        .dyn_into::<js_sys::Function>()
        .map_err(error)?;
    function
        .call2(
            &store,
            &JsValue::from_str(key),
            &value.map(JsValue::from_str).unwrap_or(JsValue::UNDEFINED),
        )
        .map_err(error)
}

fn load_draft(key: &str) -> Option<Draft> {
    #[cfg(target_arch = "wasm32")]
    {
        let text = storage("getItem", key, None).ok()?.as_string()?;
        let value = js_sys::JSON::parse(&text).ok()?;
        serde_wasm_bindgen::from_value(value).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

fn persist_draft(key: &str, draft: Option<&Draft>) -> Result<(), String> {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(draft) = draft {
            let value = serde_wasm_bindgen::to_value(draft).map_err(|e| e.to_string())?;
            let text = js_sys::JSON::stringify(&value)
                .map_err(|_| "Entwurf konnte nicht serialisiert werden".to_owned())?
                .as_string()
                .ok_or_else(|| "Entwurf konnte nicht serialisiert werden".to_owned())?;
            storage("setItem", key, Some(&text))?;
        } else {
            storage("removeItem", key, None)?;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (key, draft);
    Ok(())
}

#[derive(Clone)]
struct Editor {
    data: RwSignal<PalaceView>,
    room: RwSignal<usize>,
    selected: RwSignal<Option<String>>,
    busy: RwSignal<bool>,
    dirty: RwSignal<bool>,
    message: RwSignal<String>,
    gesture: RwSignal<Option<Gesture>>,
    paste_text: RwSignal<String>,
    history: RwSignal<History>,
    history_epoch: RwSignal<u64>,
    /// Touch-friendly undo after destructive or bulk changes (PLAN 6.3).
    toast: RwSignal<Option<(String, u64)>>,
}

#[derive(Clone)]
enum DragKind {
    Create,
    Move(LocusView),
    Resize(LocusView),
}
#[derive(Clone)]
struct Gesture {
    pointer: i32,
    start: (f64, f64),
    point: (f64, f64),
    delta: f64,
    width: f64,
    height: f64,
    delta_y: f64,
    moved: bool,
    kind: DragKind,
}

impl Editor {
    fn selected_locus(&self) -> Option<LocusView> {
        let id = self.selected.get()?;
        self.data.with(|p| {
            p.rooms
                .get(self.room.get())?
                .loci
                .iter()
                .find(|l| l.id == id)
                .cloned()
        })
    }

    fn change(&self, action: EditorAction) {
        self.apply(action, None);
    }

    fn replay(&self, redo: bool) {
        if self.dirty.get_untracked() || self.gesture.with_untracked(|g| g.is_some()) {
            return;
        }
        let action = self.history.with_untracked(|h| {
            if redo {
                h.redo.last().map(|e| e.1.clone())
            } else {
                h.undo.last().map(|e| e.0.clone())
            }
        });
        if let Some(action) = action {
            self.apply(action, Some(redo));
        }
    }

    fn apply(&self, action: EditorAction, replay: Option<bool>) {
        if self.busy.get_untracked() {
            return;
        }
        let data = self.data.get_untracked();
        let Some(room) = data.rooms.get(self.room.get_untracked()) else {
            return;
        };
        let before = inverse(&action, room);
        let after = action.clone();
        let draft_storage_key = match &action {
            EditorAction::Save { id, .. } => Some(draft_key(&data.id, &room.id, id)),
            _ => None,
        };
        let submitted_draft = draft_storage_key.as_deref().and_then(load_draft);
        let pasted = matches!(&action, EditorAction::Paste { .. });
        let toast_text = match (&action, replay) {
            (EditorAction::Delete { .. }, None) => Some("Ort gelöscht".to_owned()),
            (EditorAction::Paste { items }, None) => Some(match items.len() {
                1 => "1 Ort eingefügt".to_owned(),
                n => format!("{n} Orte eingefügt"),
            }),
            _ => None,
        };
        let change = EditorChange {
            palace_id: data.id,
            room_id: room.id.clone(),
            action,
        };
        self.busy.set(true);
        self.message.set(String::new());
        let this = self.clone();
        spawn_local(async move {
            #[derive(Serialize)]
            struct Args {
                change: EditorChange,
            }
            match call::<EditorUpdated>(CMD_EDITOR_CHANGE, Args { change }).await {
                Ok(result) => {
                    let entries = this
                        .history
                        .with_untracked(|h| (h.undo.len(), h.redo.len()));
                    if let Some(redo) = replay {
                        this.history.update(|h| {
                            if let Some(receipt) = &result.receipt {
                                let stack = if redo { &mut h.redo } else { &mut h.undo };
                                if let Some(entry) = stack.last_mut() {
                                    let action = EditorAction::Restore {
                                        receipt: receipt.clone(),
                                    };
                                    *entry = (action.clone(), action);
                                }
                            }
                            h.completed(redo);
                        });
                        this.history_epoch.update(|epoch| *epoch += 1);
                    } else if let Some(receipt) = &result.receipt {
                        this.history.update(|h| h.record_receipt(receipt.clone()));
                    } else if let Some(before) = before {
                        this.history.update(|h| h.record(before, after));
                    } else {
                        this.history.set(History::default());
                    }
                    if let Some(key) = draft_storage_key {
                        // A newer keystroke during IPC must survive this acknowledgement.
                        if load_draft(&key) == submitted_draft {
                            let _ = persist_draft(&key, None);
                        }
                    }
                    if pasted {
                        this.paste_text.set(String::new());
                    }
                    this.selected.set(result.selected);
                    this.dirty.set(false);
                    this.data.set(result.palace);
                    this.message.set("Gespeichert".into());
                    // The toast undoes the newest entry, so any newer entry or
                    // replay retires it; no-op saves leave it in place.
                    let history_changed = replay.is_some()
                        || this
                            .history
                            .with_untracked(|h| (h.undo.len(), h.redo.len()))
                            != entries;
                    match toast_text {
                        Some(text) => {
                            let token = js_sys::Date::now() as u64;
                            this.toast.set(Some((text, token)));
                            let toast = this.toast;
                            set_timeout(
                                move || {
                                    if toast.try_with_untracked(|t| t.as_ref().map(|t| t.1))
                                        == Some(Some(token))
                                    {
                                        toast.try_set(None);
                                    }
                                },
                                std::time::Duration::from_secs(8),
                            );
                        }
                        None if history_changed => this.toast.set(None),
                        None => {}
                    }
                }
                Err(error) => this.message.set(error),
            }
            this.gesture.set(None);
            this.busy.set(false);
        });
    }
}

#[component]
pub fn Rooms(
    palace: PalaceView,
    blocked: RwSignal<bool>,
    #[prop(optional)] initial_editing: bool,
) -> impl IntoView {
    let editor = Editor {
        data: RwSignal::new(palace),
        room: RwSignal::new(0),
        selected: RwSignal::new(None),
        busy: RwSignal::new(false),
        dirty: RwSignal::new(false),
        message: RwSignal::new(String::new()),
        gesture: RwSignal::new(None),
        paste_text: RwSignal::new(String::new()),
        history: RwSignal::new(History::default()),
        history_epoch: RwSignal::new(0),
        toast: RwSignal::new(None),
    };
    // Reopen the first orphaned draft, but require an explicit restore/discard.
    let recovered = editor.data.with_untracked(|p| {
        p.rooms.iter().enumerate().find_map(|(i, r)| {
            r.loci
                .iter()
                .find(|l| {
                    load_draft(&draft_key(&p.id, &r.id, &l.id))
                        .is_some_and(|draft| draft != Draft::from_locus(l))
                })
                .map(|l| (i, l.id.clone()))
        })
    });
    let editing = RwSignal::new(initial_editing || recovered.is_some());
    if let Some((room, id)) = recovered {
        editor.room.set(room);
        editor.selected.set(Some(id));
    }
    let adding = RwSignal::new(initial_editing);
    let revealed = RwSignal::new(false);
    let busy = editor.busy;
    let dirty = editor.dirty;
    let gesture = editor.gesture;
    let dragging = Memo::new(move |_| gesture.with(|g| g.is_some()));
    Effect::new(move |_| blocked.set(busy.get() || dirty.get() || dragging.get()));
    let unload = window_event_listener(leptos::ev::beforeunload, move |event| {
        if busy.get_untracked() || dirty.get_untracked() {
            event.prevent_default();
            event.set_return_value("Änderungen sind noch nicht gespeichert");
        }
    });
    on_cleanup(move || unload.remove());
    let tabs = editor.clone();
    let stage = editor.clone();
    let panel = editor.clone();
    let message = editor.message;
    let paste = editor.clone();
    let video_editor = editor.clone();
    let video_room = Memo::new(move |_| {
        video_editor.data.with(|p| {
            p.rooms
                .get(video_editor.room.get())
                .is_some_and(|r| r.video || r.backdrop.as_ref().is_some_and(|m| m.kind == locora_app_api::MediaKind::Sply))
        })
    });
    let video_time = RwSignal::new(String::from("0"));
    let playhead = RwSignal::new(0.0_f64);
    let display = crate::locus_display::signal();
    let leads = crate::locus_display::leads();
    let create_video = StoredValue::new(editor.clone());
    let shortcuts = editor.clone();
    let layer = editor.clone();
    let toast_editor = editor.clone();
    let marker_data = editor.data;
    let tab_rooms = Memo::new(move |_| {
        marker_data.with(|p| {
            p.rooms
                .iter()
                .map(|r| (r.name.clone(), r.backdrop.clone()))
                .collect::<Vec<_>>()
        })
    });
    let marker_media = Memo::new(move |_| marker_data.with(|p| p.marker.clone()));
    let drop_message = editor.message;
    let drop_selection = editor.selected;
    crate::tauri::on_file_drop(move |_| {
        if !editing.get_untracked() || drop_selection.get_untracked().is_none() {
            drop_message.set("Für ein Ortsbild bitte Bearbeiten aktivieren und einen Ort auswählen. Neue Bild-/Videoräume bitte auf der Palast-Startseite ablegen.".into());
        }
    });
    view! {
        <div class="room-editor"
            on:dragover=move |ev: web_sys::DragEvent| {
                if ev.data_transfer().is_some_and(|data| data.types().includes(&"Files".into(), 0)) {
                    ev.prevent_default();
                }
            }
            on:drop=move |ev: web_sys::DragEvent| {
                if ev.data_transfer().is_some_and(|data| data.types().includes(&"Files".into(), 0)) {
                    // Never interpret dropped filenames as trusted native paths,
                    // and do not let a browser navigate away from an unsaved draft.
                    ev.prevent_default();
                    drop_message.set("Dieser Browser-Drop enthält keine native Dateifreigabe. Bitte „Bild wählen…“ verwenden oder eine Datei im nativen App-Fenster ablegen.".into());
                }
            }
            on:keydown=move |ev: web_sys::KeyboardEvent| {
            use wasm_bindgen::JsCast;
            // Native text undo remains native; only editor-level gestures use
            // persisted history. Do not remount a form while text is unsaved.
            if ev.default_prevented() || ev.is_composing() || ev.repeat() || ev.alt_key()
                || !(ev.ctrl_key() || ev.meta_key()) || !editing.get_untracked()
                || busy.get_untracked() || dirty.get_untracked() || dragging.get_untracked() {
                return;
            }
            if ev.target().and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|element| matches!(element.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                    || element.closest("[contenteditable]").ok().flatten().is_some()) { return; }
            let key = ev.key().to_ascii_lowercase();
            if key == "z" || key == "y" {
                ev.prevent_default();
                shortcuts.replay(key == "y" || ev.shift_key());
            }
        }>
        <div class="modes">
            <label><input id="tour-editor-toggle" type="checkbox" prop:checked=move || editing.get()
                disabled=move || busy.get() || dirty.get() || dragging.get()
                on:change=move |ev| { editing.set(event_target_checked(&ev)); adding.set(false); } />"Bearbeiten"</label>
            <Show when=move || editing.get()>
                <label><input type="checkbox" bind:checked=adding disabled=move || busy.get() || dirty.get() || dragging.get() />"Ort setzen (Bühne anklicken)"</label>
                <button disabled=move || video_room.get() || busy.get() || dirty.get() || dragging.get()
                    on:click={let editor = editor.clone(); move |_| {
                        revealed.set(false);
                        editor.change(EditorAction::Create { x: 0.5, y: 0.5 });
                    }}>"Ort in der Mitte hinzufügen"</button>
                <Show when=move || video_room.get()>
                    <label>"Zeit in Sekunden "<input type="number" min="0" step="any"
                        bind:value=video_time disabled=move || busy.get() || dirty.get() || dragging.get() /></label>
                    <button disabled=move || busy.get() || dirty.get() || dragging.get()
                        on:click=move |_| {
                            let editor = create_video.get_value();
                            match video_time.get_untracked().parse::<f64>() {
                                Ok(time) if time.is_finite() && time >= 0.0 => {
                                    revealed.set(false);
                                    editor.change(EditorAction::CreateVideo { time });
                                }
                                _ => editor.message.set("Bitte eine nichtnegative Sekundenzahl eingeben".into()),
                            }
                        }>"Zeitgebundenen Ort hinzufügen"</button>
                </Show>
                <button disabled={let e = editor.clone(); move || busy.get() || dirty.get() || dragging.get() || e.history.with(|h| h.undo.is_empty())}
                    on:click={let e = editor.clone(); move |_| e.replay(false)}>"Rückgängig"</button>
                <button disabled={let e = editor.clone(); move || busy.get() || dirty.get() || dragging.get() || e.history.with(|h| h.redo.is_empty())}
                    on:click={let e = editor.clone(); move |_| e.replay(true)}>"Wiederholen"</button>
                <span>"Marker ziehen (Tastatur: Pfeiltasten) · Änderungen speichern automatisch. Rückgängig (Strg/⌘+Z): Hinzufügen, Löschen, Inhalte/Bilder, Position und Größe; ein Raumwechsel beginnt einen neuen Verlauf."</span>
            </Show>
        </div>
        <Show when=move || editing.get()>
            <ListPaste editor=paste.clone() />
        </Show>
        <div class="tabs" style:margin-top="1rem">
            {move || {
                let tabs = tabs.clone();
                // Rebuild (and reload icon frames) only when names/backdrops change.
                tab_rooms.get().into_iter().enumerate().map(|(i, (name, backdrop))| {
                    let tabs = tabs.clone();
                    view! {
                        <span class="room-tab">
                            <crate::RoomIcon backdrop=backdrop />
                            <button class:on=move || tabs.room.get() == i disabled=move || busy.get() || dirty.get() || dragging.get()
                                on:click=move |_| { tabs.history.set(History::default()); tabs.room.set(i); tabs.selected.set(None); revealed.set(false); adding.set(false); tabs.message.set(String::new()); }>
                                {name}
                            </button>
                        </span>
                    }
                }).collect_view()
            }}
        </div>
        <div class="stage-wrap">
        {move || {
            // Only rebuild the stage on room changes / committed mutations, never on pointer moves.
            let stage = stage.clone();
            let data = stage.data.get();
            let Some(room) = data.rooms.get(stage.room.get()).cloned() else {
                return view! { <p class="empty">"Dieser Palast hat noch keine Räume."</p> }.into_any();
            };
            view! { <RoomStage editor=stage room=room editing=editing adding=adding revealed=revealed playhead=playhead /> }.into_any()
        }}
        // Outside the rebuilt stage: autosave rebuilds it per keystroke, and
        // churning a frame per rebuild left new marker frames unloaded.
        {move || {
            let layer = layer.clone();
            let marker = marker_media.get();
            let loci = Signal::derive(move || {
                let preview = layer.gesture.get();
                layer.data.with(|p| {
                    p.rooms.get(layer.room.get()).map(|room| {
                        let timed = room.video || room.backdrop.as_ref().is_some_and(|m| m.kind == locora_app_api::MediaKind::Sply);
                        let prefs = display.get();
                        let overrides = leads.get();
                        let at = playhead.get();
                        let selected = layer.selected.get();
                        let hide_selected = !editing.get() && revealed.get() && prefs.hide_revealed;
                        room.loci.iter().filter(|locus| {
                            (!timed || crate::locus_display::at_video_time(locus.time, at,
                                overrides.get(&locus.id).copied().unwrap_or(prefs.video_lead)))
                                && !(hide_selected && selected.as_ref() == Some(&locus.id))
                        }).map(|locus| {
                            let (x, y) = preview_position(locus, preview.as_ref());
                            crate::sply::MarkerLocus { id: locus.id.clone(), x, y, status: locus.status.event() }
                        }).collect::<Vec<_>>()
                    }).unwrap_or_default()
                })
            });
            view! { <crate::sply::MarkerLayer media=marker loci=loci /> }
        }}
        </div>
        <p role="status" class="notes">{move || message.get()}</p>
        {
            let undo = toast_editor.clone();
            move || undo.toast.get().map(|(text, _)| {
                let undo = undo.clone();
                view! {
                    <div class="toast" role="status">
                        <span>{text}</span>
                        <button disabled=move || busy.get() || dirty.get() on:click=move |_| undo.replay(false)>"Rückgängig"</button>
                    </div>
                }
            })
        }
        {move || {
            let panel = panel.clone();
            if editing.get() {
                let items = panel.clone();
                view! { <For
                    each={move || items.selected_locus().into_iter().map(|l| (l, items.history_epoch.get())).collect::<Vec<_>>()}
                    key=|(l, epoch)| (l.id.clone(), *epoch)
                    children={move |(l, _)| view! { <ContentForm locus=l editor=panel.clone() revealed=revealed adding=adding /> }}
                /> }.into_any()
            } else {
                panel.selected_locus().map(|locus| {
                    let answer = StoredValue::new(locus.clone());
                    view! {
                        <section class="card panel">
                            <div class="progress">{format!("Ort {}", locus.number)}</div>
                            <div class="prompt">{locus.place}</div>
                            <button on:click=move |_| revealed.set(!revealed.get())>{move || if revealed.get() { "Verbergen" } else { "Aufdecken" }}</button>
                            <Show when=move || revealed.get()>{move || {
                                let l = answer.get_value();
                                view! { <Answer solution=l.item note=l.note picture=l.picture /> }
                            }}</Show>
                        </section>
                    }.into_any()
                }).into_any()
            }
        }}
        </div>
    }
}

#[component]
fn ListPaste(editor: Editor) -> impl IntoView {
    let text = editor.paste_text;
    let busy = editor.busy;
    let dirty = editor.dirty;
    let gesture = editor.gesture;
    let check = editor.clone();
    let unavailable = Memo::new(move |_| {
        check
            .data
            .with(|p| p.rooms.get(check.room.get()).is_none_or(|room| room.video))
    });
    view! {
        // Collapsed by default so the stage stays above the fold; Ctrl/⌘+V on
        // the focused stage works without opening it.
        <details class="card panel editor-form list-paste">
            <summary>"Liste als Route einfügen (oder Strg/⌘+V auf der Bühne)"</summary>
            <label>"Ein Begriff pro Zeile"
                <textarea rows="4" bind:value=text maxlength="65536"
                    disabled=move || busy.get() || dirty.get() || gesture.with(|g| g.is_some()) || unavailable.get() />
            </label>
            <p class="notes">"1–100 Begriffe, je höchstens 500 Bytes. Leerzeilen werden ignoriert. Neue Orte werden als Schlangenroute angehängt; vorhandene Orte bleiben unverändert. Positionen danach per Ziehen korrigieren. Nicht für Video-Räume."</p>
            <button disabled=move || busy.get() || dirty.get() || gesture.with(|g| g.is_some()) || unavailable.get()
                on:click=move |_| {
                    match parse_list(&text.get_untracked()) {
                        Ok(items) => editor.change(EditorAction::Paste { items }),
                        Err(error) => editor.message.set(error),
                    }
                }>"Liste als Route hinzufügen"</button>
        </details>
    }
}

#[component]
fn RoomStage(
    editor: Editor,
    room: locora_app_api::RoomView,
    editing: RwSignal<bool>,
    adding: RwSignal<bool>,
    revealed: RwSignal<bool>,
    playhead: RwSignal<f64>,
) -> impl IntoView {
    // The media element starts at zero after a committed edit remounts this stage.
    playhead.set(0.0);
    let node = NodeRef::<html::Div>::new();
    let busy = editor.busy;
    let dirty = editor.dirty;
    let gesture = editor.gesture;
    let begin = {
        let editor = editor.clone();
        move |ev: PointerEvent, kind: DragKind| {
            if busy.get_untracked()
                || dirty.get_untracked()
                || gesture.get_untracked().is_some()
                || !ev.is_primary()
                || ev.button() != 0
            {
                return;
            }
            let Some(stage) = node.get() else {
                return;
            };
            let rect = stage.get_bounding_client_rect();
            let Some(point) = stage_point(
                ev.client_x() as f64,
                ev.client_y() as f64,
                rect.left(),
                rect.top(),
                rect.width(),
                rect.height(),
            ) else {
                return;
            };
            if stage.set_pointer_capture(ev.pointer_id()).is_err() {
                return;
            }
            ev.prevent_default();
            if let DragKind::Move(locus) | DragKind::Resize(locus) = &kind
                && editor.selected.get_untracked().as_ref() != Some(&locus.id)
            {
                revealed.set(false);
                editor.selected.set(Some(locus.id.clone()));
            }
            gesture.set(Some(Gesture {
                pointer: ev.pointer_id(),
                start: (ev.client_x() as f64, ev.client_y() as f64),
                point,
                delta: 0.0,
                width: rect.width(),
                height: rect.height(),
                delta_y: 0.0,
                moved: false,
                kind,
            }));
        }
    };
    let begin = StoredValue::new_local(begin);
    let update = move |ev: &PointerEvent| {
        let Some(stage) = node.get() else {
            return;
        };
        let rect = stage.get_bounding_client_rect();
        gesture.update(|value| {
            if let Some(g) = value.as_mut().filter(|g| g.pointer == ev.pointer_id()) {
                let x = ev.client_x() as f64;
                let y = ev.client_y() as f64;
                if let Some(point) =
                    stage_point(x, y, rect.left(), rect.top(), rect.width(), rect.height())
                {
                    g.point = point;
                    g.delta = x - g.start.0;
                    g.delta_y = y - g.start.1;
                    g.moved |= (x - g.start.0).hypot(y - g.start.1) >= 4.0;
                }
            }
        });
    };
    let finish = editor.clone();
    let picture = editor.clone();
    let pick_editor = editor.clone();
    let paste_editor = editor.clone();
    let video = room.video;
    let timed_sply = !video && room.backdrop.as_ref().is_some_and(|media| media.kind == locora_app_api::MediaKind::Sply);
    let timed = video || timed_sply;
    let display = crate::locus_display::signal();
    let leads = crate::locus_display::leads();
    let player = NodeRef::<html::Video>::new();
    let video_duration = RwSignal::new(None::<f64>);
    let sply_command = RwSignal::new(TimedCommand { serial: 0, time: 0.0, start: None, play: false });
    let mut timeline: Vec<_> = room
        .loci
        .iter()
        .filter_map(|l| {
            l.time
                .filter(|t| t.is_finite() && *t >= 0.0)
                .map(|time| (time, l.id.clone(), l.number, l.place.clone()))
        })
        .collect();
    timeline.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
    let timeline_marks = timeline.clone();
    let timeline_editor = StoredValue::new(editor.clone());
    let timed_editor = editor.clone();
    let stepped_editor = editor.clone();
    let create_at_playhead = StoredValue::new_local(move || {
        if !editing.get_untracked()
            || busy.get_untracked()
            || dirty.get_untracked()
            || gesture.with_untracked(|g| g.is_some())
        {
            return;
        }
        let time = if timed_sply {
            if video_duration.get_untracked().is_none() {
                timed_editor.message.set("Die Animation ist noch nicht geladen".into());
                return;
            }
            playhead.get_untracked()
        } else {
            let Some(player) = player.get() else { return; };
            if player.ready_state() == 0 {
                timed_editor.message.set("Das Video ist noch nicht geladen".into());
                return;
            }
            if player.pause().is_err() {
                timed_editor.message.set("Video konnte nicht angehalten werden".into());
                return;
            }
            player.current_time()
        };
        if !time.is_finite() || time < 0.0 {
            return;
        }
        if timed_sply {
            sply_command.update(|command| {
                command.serial += 1;
                command.time = time;
                command.start = None;
                command.play = false;
            });
        }
        revealed.set(false);
        timed_editor.change(EditorAction::CreateVideo { time });
    });
    let step_frame = StoredValue::new_local(move |forward: bool| {
        if !editing.get_untracked()
            || busy.get_untracked()
            || gesture.with_untracked(|g| g.is_some())
        {
            return;
        }
        let Some(player) = player.get() else {
            return;
        };
        if player.ready_state() == 0 {
            stepped_editor
                .message
                .set("Das Video ist noch nicht geladen".into());
            return;
        }
        if player.pause().is_err() {
            stepped_editor
                .message
                .set("Video konnte nicht angehalten werden".into());
            return;
        }
        let duration = player.duration();
        player.set_current_time(frame_step_time(
            player.current_time(),
            forward,
            duration.is_finite().then_some(duration),
        ));
    });
    view! {
        <Show when=move || editing.get() && timed>
            <p class="notes">{if video { "Video: Zeitpunkt auf der Zeitleiste wählen, „Ort setzen“ aktivieren und ins Bild klicken. Taste L setzt einen Ort in der Mitte; , und . springen einen Frame." } else { "Splinely: Zeitpunkt auf der Zeitleiste wählen, „Ort setzen“ aktivieren und ins Bild klicken." }}</p>
            <button disabled=move || busy.get() || dirty.get() || gesture.with(|g| g.is_some())
                on:click=move |_| create_at_playhead.with_value(|create| create())>
                "Ort an aktueller Zeit"
            </button>
        </Show>
        {if timed {
            Some(view! {
                <nav class="video-timeline" aria-label="Orte nach Zeit">
                    <Show when=move || video_duration.get().is_some()>
                        <div class="video-timeline-scale" aria-hidden="true">
                            {timeline_marks.clone().into_iter().map(|(time, _, _, _)| view! {
                                <span class="video-timeline-mark"
                                    class:invalid=move || video_duration.get().is_some_and(|duration| time > duration)
                                    style:left=move || video_duration.get()
                                        .map(|duration| format!("{}%", (time / duration * 100.0).clamp(0.0, 100.0)))
                                        .unwrap_or_default()
                                ></span>
                            }).collect_view()}
                        </div>
                        <div class="video-timeline-ends" aria-hidden="true">
                            <span>"0 s"</span>
                            <span>{move || format!("{:.2} s", video_duration.get().unwrap_or_default())}</span>
                        </div>
                    </Show>
                    <p class="notes video-playhead">{move || format!("Position {:.2} s", playhead.get())}</p>
                    <label class="video-seek">"Zeitpunkt wählen"
                        <input type="range" min="0" step="0.01"
                            max=move || video_duration.get().unwrap_or(0.0).to_string()
                            prop:value=move || playhead.get().to_string()
                            disabled=move || video_duration.get().is_none_or(|d| !d.is_finite() || d <= 0.0)
                            on:input=move |ev| {
                                let Ok(time) = event_target_value(&ev).parse::<f64>() else { return; };
                                let Some(duration) = video_duration.get_untracked() else { return; };
                                if !time.is_finite() || !(0.0..=duration).contains(&time) { return; }
                                if timed_sply {
                                    sply_command.update(|command| { command.serial += 1; command.time = time; command.start = None; command.play = false; });
                                } else if let Some(video) = player.get_untracked() {
                                    let _ = video.pause();
                                    video.set_current_time(time);
                                }
                                playhead.set(time);
                            }
                        />
                    </label>
                    {timed_sply.then(|| view! {
                        <div class="row">
                            <button disabled=move || video_duration.get().is_none()
                                on:click=move |_| {
                                    if let Some(duration) = video_duration.get_untracked() {
                                        sply_command.update(|command| {
                                            command.serial += 1;
                                            command.time = duration;
                                            command.start = Some(0.0);
                                            command.play = true;
                                        });
                                    }
                                }>"Animation abspielen"</button>
                            <button disabled=move || video_duration.get().is_none()
                                on:click=move |_| {
                                    let time = playhead.get_untracked();
                                    sply_command.update(|command| {
                                        command.serial += 1;
                                        command.time = time;
                                        command.start = None;
                                        command.play = false;
                                    });
                                }>"Pause"</button>
                        </div>
                    })}
                    <div class="video-timeline-loci">
                    {timeline.into_iter().map(|(time, id, number, place)| {
                        let selected_id = id.clone();
                        let retime_id = id.clone();
                        let lead_id = id.clone();
                        let lead_write_id = id.clone();
                        view! {
                            <div class="video-timeline-locus">
                            <button disabled=move || busy.get() || dirty.get() || gesture.with(|g| g.is_some())
                                class:on=move || timeline_editor.with_value(|e| e.selected.get().as_ref() == Some(&selected_id))
                                class:invalid=move || video_duration.get().is_some_and(|duration| time > duration)
                                on:click=move |_| {
                                    let editor = timeline_editor.get_value();
                                    if busy.get_untracked() || dirty.get_untracked() || gesture.with_untracked(|g| g.is_some()) { return; }
                                    if timed_sply {
                                        let Some(duration) = video_duration.get_untracked() else {
                                            editor.message.set("Die Animation ist noch nicht geladen".into());
                                            return;
                                        };
                                        if time > duration {
                                            editor.message.set("Der Ort liegt hinter dem Ende der Animation".into());
                                            return;
                                        }
                                        sply_command.update(|command| {
                                            command.serial += 1;
                                            command.time = time;
                                            command.start = None;
                                            command.play = false;
                                        });
                                    } else {
                                        let Some(player) = player.get() else { return; };
                                        if player.ready_state() == 0 {
                                            editor.message.set("Das Video ist noch nicht geladen".into());
                                            return;
                                        }
                                        let duration = player.duration();
                                        if duration.is_finite() && time > duration {
                                            editor.message.set("Der Ort liegt hinter dem Ende des Videos".into());
                                            return;
                                        }
                                        if player.pause().is_err() {
                                            editor.message.set("Video konnte nicht angehalten werden".into());
                                            return;
                                        }
                                        player.set_current_time(time);
                                    }
                                    revealed.set(false);
                                    editor.selected.set(Some(id.clone()));
                                }>
                                {format!("{time:.2} s · {number}. {place}")}
                            </button>
                            <input type="range" min="0" step="0.033333333"
                                max=move || video_duration.get().unwrap_or(time).max(0.001).to_string()
                                prop:value=time.to_string()
                                disabled=move || !editing.get() || busy.get() || dirty.get() || video_duration.get().is_none_or(|d| !d.is_finite() || d <= 0.0)
                                aria-label=format!("Zeitpunkt von Ort {number} verschieben")
                                on:change=move |ev| {
                                    let Ok(time) = event_target_value(&ev).parse::<f64>() else { return; };
                                    let Some(duration) = video_duration.get_untracked() else { return; };
                                    if !time.is_finite() || !duration.is_finite() || !(0.0..=duration).contains(&time) { return; }
                                    let editor = timeline_editor.get_value();
                                    editor.change(EditorAction::Retime { id: retime_id.clone(), time });
                                }
                            />
                            <label>"Marker vorher (s)"
                                <input type="number" min="0" max="30" step="0.1"
                                    prop:value=move || leads.get().get(&lead_id).copied().unwrap_or(display.get().video_lead).to_string()
                                    on:change=move |ev| {
                                        if let Ok(value) = event_target_value(&ev).parse::<f64>() {
                                            if value.is_finite() && (0.0..=30.0).contains(&value) {
                                                leads.update(|values| { values.insert(lead_write_id.clone(), value); });
                                                crate::locus_display::save_leads(&leads.get_untracked());
                                            }
                                        }
                                    } />
                            </label>
                            </div>
                        }
                    }).collect_view()}
                    </div>
                </nav>
            })
        } else { None }}
        <div class="stage" class:editing=move || editing.get() node_ref=node tabindex="0"
            aria-label=if video { "Videoraum – Bühne fokussieren: L setzt einen Ort an der aktuellen Zeit, , und . springen einen Frame" } else if timed_sply { "Animationsraum – Bühne fokussieren: L setzt einen Ort an der aktuellen Zeit" } else { "Raum – im Bearbeitungsmodus mit Strg+V eine Liste als Route einfügen" }
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                // Only the stage itself, never native video controls or markers.
                let stage_focused = ev.target().is_some() && ev.target() == ev.current_target();
                if timed && stage_focused && editing.get_untracked() && !ev.repeat()
                    && !ev.is_composing() && !ev.ctrl_key() && !ev.meta_key()
                    && !ev.alt_key() && ev.key().eq_ignore_ascii_case("l") {
                    ev.prevent_default();
                    create_at_playhead.with_value(|create| create());
                }
                let frame_key = ev.key();
                if video && stage_focused && editing.get_untracked() && !ev.is_composing()
                    && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() && (frame_key == "," || frame_key == ".") {
                    ev.prevent_default();
                    step_frame.with_value(|step| step(frame_key == "."));
                }
            }
            on:paste=move |ev: web_sys::ClipboardEvent| {
                // Text fields are outside the stage, so ordinary text paste
                // never reaches this handler. Do not request clipboard access.
                if !editing.get_untracked() || timed || busy.get_untracked() || dirty.get_untracked() || gesture.with_untracked(|g| g.is_some()) {
                    return;
                }
                let Some(data) = ev.clipboard_data() else { return; };
                ev.prevent_default();
                match data.get_data("text/plain").map_err(|_| "Zwischenablage konnte nicht gelesen werden".to_owned()).and_then(|text| parse_list(&text)) {
                    Ok(items) => {
                        revealed.set(false);
                        paste_editor.change(EditorAction::Paste { items });
                    }
                    Err(error) => paste_editor.message.set(error),
                }
            }
            on:pointerdown=move |ev| {
                if editing.get() && adding.get() { begin.with_value(|begin| begin(ev, DragKind::Create)); }
            }
            on:pointermove=move |ev| { if !busy.get_untracked() { update(&ev); } }
            on:pointerup=move |ev| {
                if busy.get_untracked() { return; }
                update(&ev);
                let Some(g) = gesture.get_untracked().filter(|g| g.pointer == ev.pointer_id()) else { return; };
                if let Some(stage) = node.get() { let _ = stage.release_pointer_capture(g.pointer); }
                let action = match g.kind {
                    DragKind::Create if !g.moved => {
                        revealed.set(false);
                        if timed {
                            let time = if timed_sply {
                                let time = playhead.get_untracked();
                                sply_command.update(|command| { command.serial += 1; command.time = time; command.start = None; command.play = false; });
                                time
                            } else if let Some(video) = player.get_untracked() {
                                let _ = video.pause();
                                video.current_time()
                            } else { playhead.get_untracked() };
                            playhead.set(time);
                            if video_duration.get_untracked().is_none() {
                                finish.message.set("Zeitgesteuertes Medium ist noch nicht geladen".into());
                                None
                            } else {
                                Some(EditorAction::CreateTimed { time, x: g.point.0, y: g.point.1 })
                            }
                        } else { Some(EditorAction::Create { x: g.point.0, y: g.point.1 }) }
                    }
                    DragKind::Move(ref l) if g.moved => {
                        let (x, y) = preview_position(l, Some(&g));
                        Some(EditorAction::Move { id: l.id.clone(), x, y })
                    },
                    DragKind::Resize(l) if g.moved => Some(EditorAction::Resize { id: l.id, image_size: resized(l.image_size, g.delta, g.width) }),
                    _ => None,
                };
                if let Some(action) = action { finish.change(action); } else { gesture.set(None); }
            }
            on:pointercancel=move |ev: PointerEvent| {
                if !busy.get_untracked() && gesture.with_untracked(|g| g.as_ref().is_some_and(|g| g.pointer == ev.pointer_id())) { gesture.set(None); }
            }
            on:lostpointercapture=move |ev: PointerEvent| {
                // Normal pointerup releases capture before the asynchronous save.
                if !busy.get_untracked() && gesture.with_untracked(|g| g.as_ref().is_some_and(|g| g.pointer == ev.pointer_id())) { gesture.set(None); }
            }
        >
            {room.backdrop.map(|media| {
                if video {
                    view! { <VideoBackdrop media=media class="backdrop" controls=true muted=true node_ref=player
                        on_loaded=Box::new(move || {
                            video_duration.set(player.get_untracked().map(|video| video.duration())
                                .filter(|duration| duration.is_finite() && *duration > 0.0));
                            if let Some(video) = player.get_untracked() {
                                fit_stage(video.as_ref(), f64::from(video.video_width()), f64::from(video.video_height()));
                            }
                        })
                        on_timeupdate=Box::new(move || {
                            if let Some(video) = player.get_untracked() {
                                playhead.set(video.current_time());
                            }
                        })
                    /> }.into_any()
                } else if timed_sply {
                    view! { <Sply media=media class="backdrop" timed=sply_command
                        on_timed_ready=Box::new(move |duration| {
                            video_duration.set(Some(duration));
                        })
                        on_timed_position=Box::new(move |time| playhead.set(time))
                        on_timed_at=Box::new(move |time| playhead.set(time))
                        on_timed_error=Box::new({ let message = editor.message; move || message.set("Splinely-Zeitleiste konnte nicht gesteuert werden".into()) })
                    /> }.into_any()
                } else {
                    view! { <Shown media=media class="backdrop" /> }.into_any()
                }
            })}
            {move || {
                if !revealed.get() { return None; }
                let l = picture.selected_locus()?;
                let media = l.picture.clone()?;
                let size_locus = l.clone();
                let position_locus = l.clone();
                let top_locus = l.clone();
                let resize_locus = l.clone();
                Some(view! {
                    <div class="locus-picture"
                        style:left=move || format!("{}%", preview_position(&position_locus, gesture.get().as_ref()).0 * 100.0)
                        style:top=move || format!("{}%", preview_position(&top_locus, gesture.get().as_ref()).1 * 100.0)
                        style:width=move || {
                            let size = match gesture.get() {
                                Some(Gesture { kind: DragKind::Resize(ref original), delta, width, .. }) if original.id == size_locus.id => resized(original.image_size, delta, width),
                                _ => size_locus.image_size,
                            };
                            format!("{}%", size * 100.0)
                        }
                    >
                        <Shown media=media />
                        <Show when=move || editing.get()>
                            <button class="resize-handle" title="Bildgröße ziehen" aria-label="Bildgröße ziehen"
                                disabled=move || busy.get() || dirty.get()
                                on:pointerdown={let l = resize_locus.clone(); move |ev: PointerEvent| { ev.stop_propagation(); begin.with_value(|begin| begin(ev, DragKind::Resize(l.clone()))); }}
                            >"↔"</button>
                        </Show>
                    </div>
                })
            }}
            {room.loci.into_iter().map(|locus| {
                let editor = pick_editor.clone();
                let keyboard_editor = editor.clone();
                let selected = editor.selected;
                let l = StoredValue::new(locus.clone());
                let fixed = locus.is_bound("x") || locus.is_bound("y");
                view! {
                    <div class="marker"
                        style:display=move || {
                            let locus = l.get_value();
                            let lead = leads.get().get(&locus.id).copied().unwrap_or(display.get().video_lead);
                            let visible_at_time = !timed || crate::locus_display::at_video_time(locus.time, playhead.get(), lead);
                            let hide_revealed = !editing.get() && revealed.get() && display.get().hide_revealed
                                && selected.get().as_ref() == Some(&locus.id);
                            if visible_at_time && !hide_revealed { "block" } else { "none" }
                        }
                        style:left=move || format!("{}%", preview_position(&l.get_value(), gesture.get().as_ref()).0 * 100.0)
                        style:top=move || format!("{}%", preview_position(&l.get_value(), gesture.get().as_ref()).1 * 100.0)
                    >
                        <button title=format!("Ort {}: {}", locus.number, locus.place)
                            disabled=move || busy.get() || dirty.get()
                            on:pointerdown=move |ev: PointerEvent| {
                                ev.stop_propagation();
                                // A formula-bound position cannot be dragged; clicking still selects.
                                if editing.get() && !fixed { begin.with_value(|begin| begin(ev, DragKind::Move(l.get_value()))); }
                            }
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if !editing.get() || fixed || busy.get_untracked() || dirty.get_untracked() { return; }
                                let step = if ev.shift_key() { 0.1 } else { 0.01 };
                                let delta = match ev.key().as_str() {
                                    "ArrowLeft" => (-step, 0.0), "ArrowRight" => (step, 0.0),
                                    "ArrowUp" => (0.0, -step), "ArrowDown" => (0.0, step), _ => return,
                                };
                                ev.prevent_default();
                                let locus = l.get_value();
                                let (x, y) = moved_position((locus.x, locus.y), delta);
                                keyboard_editor.change(EditorAction::Move { id: locus.id, x, y });
                            }
                            on:click=move |ev| {
                                ev.stop_propagation();
                                if !busy.get_untracked() && !dirty.get_untracked() && editor.selected.get_untracked().as_ref() != Some(&l.get_value().id) {
                                    editor.selected.set(Some(l.get_value().id)); revealed.set(false);
                                }
                            }
                        ></button>
                        <span class="num" aria-hidden="true" style:display=move || if editing.get() || display.get().numbers { "grid" } else { "none" }>{locus.number}</span>
                    </div>
                }
            }).collect_view()}
        </div>
    }
}

fn moved_position(original: (f64, f64), delta: (f64, f64)) -> (f64, f64) {
    (
        (original.0 + delta.0).clamp(0.0, 1.0),
        (original.1 + delta.1).clamp(0.0, 1.0),
    )
}

fn preview_position(locus: &LocusView, gesture: Option<&Gesture>) -> (f64, f64) {
    match gesture {
        Some(Gesture {
            kind: DragKind::Move(original),
            delta,
            delta_y,
            width,
            height,
            moved: true,
            ..
        }) if original.id == locus.id => moved_position(
            (original.x, original.y),
            (*delta / *width, *delta_y / *height),
        ),
        _ => (locus.x, locus.y),
    }
}

#[component]
fn ContentForm(
    locus: LocusView,
    editor: Editor,
    revealed: RwSignal<bool>,
    adding: RwSignal<bool>,
) -> impl IntoView {
    let name_field = NodeRef::<html::Input>::new();
    let place = RwSignal::new(locus.place.clone());
    Effect::new(move |_| {
        if let Some(input) = name_field.get()
            && adding.get_untracked()
        {
            // The bound value may not be applied yet; set it first so typing
            // replaces the default name instead of appending to it.
            input.set_value(&place.get_untracked());
            let _ = input.focus();
            input.select();
        }
    });
    let finish_requested = RwSignal::new(false);
    let place_bound = locus.is_bound("place");
    let item_bound = locus.is_bound("item");
    let bound_note = {
        let fields: Vec<&str> = [
            ("place", "Ort"),
            ("item", "Inhalt"),
            ("x", "x"),
            ("y", "y"),
            ("visible", "Sichtbar"),
            ("active", "Aktiv"),
        ]
        .into_iter()
        .filter(|(field, _)| locus.is_bound(field))
        .map(|(_, label)| label)
        .collect();
        format!(
            "Workbook-Ort: {} kommt aus Formeln und wird in der Quelldatei geändert; alles andere ist hier bearbeitbar.",
            fields.join(", ")
        )
    };
    let item = RwSignal::new(locus.item.clone());
    let note = RwSignal::new(locus.note.clone());
    let image = RwSignal::new(locus.image.clone().unwrap_or_default());
    let size = RwSignal::new(locus.image_size.to_string());
    let zoom = RwSignal::new(locus.zoom.to_string());
    let initial_kind = match locus.answer.kind {
        EditorAnswerKind::Recall => "recall",
        EditorAnswerKind::Guess => "guess",
        EditorAnswerKind::Quiz => "quiz",
    };
    let kind = RwSignal::new(initial_kind.to_owned());
    let accept = RwSignal::new(locus.answer.accept.join("\n"));
    let quiz = RwSignal::new(
        locus
            .answer
            .options
            .iter()
            .map(|o| format!("{} {}", if o.correct { "+" } else { "-" }, o.text))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let key = editor
        .data
        .with_untracked(|p| draft_key(&p.id, &p.rooms[editor.room.get_untracked()].id, &locus.id));
    let recovery = RwSignal::new(recoverable_draft(
        load_draft(&key),
        &Draft::from_locus(&locus),
    ));
    let storage_key = StoredValue::new(key);
    let storage_error = RwSignal::new(String::new());
    let dirty = editor.dirty;
    if recovery.get_untracked().is_some() {
        dirty.set(true);
    }
    let busy = editor.busy;
    let confirm_delete = RwSignal::new(false);
    let gesture = editor.gesture;
    let dragging = Memo::new(move |_| gesture.with(|g| g.is_some()));
    let save = editor.clone();
    let last_attempted = StoredValue::new(Draft::from_locus(&locus));
    // Storage revisions are independent of IPC attempts: A -> B -> A while
    // saving A must replace B on disk, even though A was already attempted.
    let last_persisted = StoredValue::new(Draft::from_locus(&locus));
    let committed_size = RwSignal::new(locus.image_size);
    let projection = editor.clone();
    Effect::new(move |_| {
        let Some(updated) = projection.selected_locus() else {
            return;
        };
        let previous = committed_size.get_untracked();
        if updated.image_size != previous {
            committed_size.set(updated.image_size);
            if let Some(value) =
                synchronized_size(&size.get_untracked(), previous, updated.image_size)
            {
                // A geometry-only resize must not be undone by the next content
                // save from this stable form. Preserve newer typed size drafts.
                last_attempted.update_value(|draft| draft.size = value.clone());
                // This revision was already committed by the resize command;
                // it must not reappear as a crash-recovery draft after Undo.
                last_persisted.update_value(|draft| draft.size = value.clone());
                if last_attempted.with_value(|draft| draft.content().ok())
                    == Draft::from_locus(&updated).content().ok()
                    && let Err(error) = persist_draft(&storage_key.get_value(), None)
                {
                    storage_error.set(error);
                }
                size.set(value);
            }
        }
    });
    let save_id = locus.id.clone();
    Effect::new(move |_| {
        let draft = Draft {
            place: place.get(),
            item: item.get(),
            note: note.get(),
            image: image.get(),
            size: size.get(),
            zoom: zoom.get(),
            kind: kind.get(),
            accept: accept.get(),
            quiz: quiz.get(),
        };
        if recovery.get().is_some() {
            return;
        }
        if last_persisted.with_value(|old| old != &draft) {
            // Persist every raw revision, including reversions, before waiting
            // for IPC. Invalid numbers/quiz lines are equally important.
            match persist_draft(&storage_key.get_value(), Some(&draft)) {
                Ok(()) => {
                    last_persisted.set_value(draft.clone());
                    storage_error.set(String::new());
                }
                Err(error) => storage_error.set(error),
            }
        }
        // Serialize saves, but keep the draft editable during IPC. Once the
        // current write finishes this effect sends the newest draft, not an
        // older queued keystroke. A rejected revision is not retried forever.
        if busy.get() || dragging.get() || last_attempted.with_value(|old| old == &draft) {
            return;
        }
        last_attempted.set_value(draft.clone());
        dirty.set(true);
        match draft.content() {
            Ok(content) => save.change(EditorAction::Save {
                id: save_id.clone(),
                content,
            }),
            Err(error) => save.message.set(format!("Noch nicht gespeichert: {error}")),
        }
    });
    let finish = editor.clone();
    Effect::new(move |_| {
        if !finish_requested.get() || finish.busy.get() || finish.dirty.get() {
            return;
        }
        let draft = Draft {
            place: place.get(),
            item: item.get(),
            note: note.get(),
            image: image.get(),
            size: size.get(),
            zoom: zoom.get(),
            kind: kind.get(),
            accept: accept.get(),
            quiz: quiz.get(),
        };
        let Ok(content) = draft.content() else {
            finish_requested.set(false);
            return;
        };
        // Wait for the latest keystroke, not just an earlier IPC write, to commit.
        if finish
            .selected_locus()
            .and_then(|l| Draft::from_locus(&l).content().ok())
            == Some(content)
        {
            adding.set(true);
            finish.selected.set(None);
        }
    });
    let picking = RwSignal::new(false);
    let picker = editor.clone();
    let pick_id = locus.id.clone();
    let import_picture = StoredValue::new(move |ticket: Option<String>| {
        if busy.get_untracked() || dirty.get_untracked() || dragging.get_untracked() {
            if ticket.is_some() {
                picker.message.set("Datei nicht übernommen: erst Änderungen speichern/verwerfen und einen bearbeitbaren Ort auswählen.".into());
            }
            return;
        }
        let data = picker.data.get_untracked();
        let Some(room) = data.rooms.get(picker.room.get_untracked()) else {
            return;
        };
        let target = locora_app_api::PictureTarget {
            palace_id: data.id.clone(),
            room_id: room.id.clone(),
            locus_id: pick_id.clone(),
        };
        let before = picker
            .selected_locus()
            .and_then(|l| Draft::from_locus(&l).content().ok());
        let history_id = pick_id.clone();
        let this = picker.clone();
        picking.set(true);
        busy.set(true);
        this.message.set(String::new());
        spawn_local(async move {
            #[derive(Serialize)]
            struct Args {
                target: locora_app_api::PictureTarget,
            }
            let result = if let Some(ticket) = ticket {
                #[derive(Serialize)]
                struct DropArgs {
                    ticket: String,
                    target: locora_app_api::PictureTarget,
                }
                call::<EditorUpdated>("editor_import_drop", DropArgs { ticket, target })
                    .await
                    .map(Some)
            } else {
                call::<Option<EditorUpdated>>(
                    locora_app_api::CMD_EDITOR_PICK_PICTURE,
                    Args { target },
                )
                .await
            };
            match result {
                Ok(Some(result)) => {
                    let updated = result
                        .palace
                        .rooms
                        .iter()
                        .flat_map(|r| &r.loci)
                        .find(|l| l.id == history_id);
                    if let (Some(before), Some(after)) = (
                        before,
                        updated.and_then(|l| Draft::from_locus(l).content().ok()),
                    ) {
                        this.history.update(|h| {
                            h.record(
                                EditorAction::Save {
                                    id: history_id.clone(),
                                    content: before,
                                },
                                EditorAction::Save {
                                    id: history_id,
                                    content: after,
                                },
                            )
                        });
                    }
                    let hash = result
                        .palace
                        .rooms
                        .iter()
                        .flat_map(|r| &r.loci)
                        .find(|l| Some(&l.id) == result.selected.as_ref())
                        .and_then(|l| l.image.clone())
                        .unwrap_or_default();
                    last_attempted.update_value(|draft| draft.image = hash.clone());
                    image.set(hash);
                    this.data.set(result.palace);
                    this.message.set("Bild gespeichert".into());
                }
                Ok(None) => {}
                Err(error) => this.message.set(error),
            }
            picking.set(false);
            busy.set(false);
        });
    });
    let drop_error = editor.message;
    crate::tauri::on_file_drop(move |drop| match drop {
        Ok(ticket) => import_picture.with_value(|import| import(Some(ticket))),
        Err(error) => drop_error.set(error),
    });
    let pick = move |_| import_picture.with_value(|import| import(None));
    let delete = editor.clone();
    let cancel = editor.clone();
    let id = StoredValue::new(locus.id.clone());
    let restore_fields = StoredValue::new(move |draft: Draft| {
        place.set(draft.place);
        item.set(draft.item);
        note.set(draft.note);
        image.set(draft.image);
        size.set(draft.size);
        zoom.set(draft.zoom);
        kind.set(draft.kind);
        accept.set(draft.accept);
        quiz.set(draft.quiz);
    });
    view! {
        <form class="card panel editor-form" on:submit=move |ev| ev.prevent_default()
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                let single_line = ev.target().and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(t).ok()).is_some();
                if single_line && ev.key() == "Enter" && !ev.is_composing() && !ev.ctrl_key() && !ev.meta_key() && !ev.alt_key() && recovery.get_untracked().is_none() {
                    ev.prevent_default();
                    finish_requested.set(true);
                }
            }>
            <div class="progress">{format!("Ort {} bearbeiten", locus.number)}</div>
            <p class="notes">"Ein Ortsbild kann auch im nativen App-Fenster abgelegt werden (ausgewählter Ort, keine offenen Änderungen). Videos als neue Räume bitte auf der Palast-Startseite ablegen."</p>
            <Show when=move || recovery.get().is_some()>
                <p role="alert">"Ein lokaler, noch nicht gespeicherter Entwurf wurde gefunden. Wiederherstellen speichert gültige Inhalte automatisch."</p>
                <button type="button" disabled=move || busy.get() || dragging.get() on:click=move |_| {
                    if let Some(draft) = recovery.get_untracked() {
                        restore_fields.with_value(|restore| restore(draft));
                        recovery.set(None);
                    }
                }>"Entwurf wiederherstellen"</button>
                <button type="button" disabled=move || busy.get() || dragging.get() on:click=move |_| {
                    match persist_draft(&storage_key.get_value(), None) {
                        Ok(()) => { recovery.set(None); dirty.set(false); storage_error.set(String::new()); }
                        Err(error) => storage_error.set(error),
                    }
                }>"Lokalen Entwurf verwerfen"</button>
            </Show>
            <Show when=move || !storage_error.get().is_empty()><p role="alert">{move || storage_error.get()}</p></Show>
            <Show when=move || locus.bound><p class="notes">{bound_note.clone()}</p></Show>
            <fieldset disabled=move || recovery.get().is_some() || picking.get() || dragging.get()
                on:input=move |_| confirm_delete.set(false)
                on:change=move |_| confirm_delete.set(false)>
                <label>"Ort"<input type="text" node_ref=name_field bind:value=place disabled=place_bound /></label>
                <label>"Inhalt"<input type="text" bind:value=item disabled=item_bound /></label>
                <label>"Merkhilfe"<textarea bind:value=note /></label>
                <div class="tabs">
                    <button type="button" disabled=move || busy.get() || dirty.get() || dragging.get() on:click=pick>"Bild wählen…"</button>
                    <button type="button" disabled=move || busy.get() || dirty.get() || dragging.get() || image.get().is_empty()
                        on:click=move |_| image.set(String::new())>"Bild entfernen"</button>
                </div>
                <label>"Bildgröße (Anteil der Raumbreite, 0.05–1)"<input type="number" min="0.05" max="1" step="any" bind:value=size /></label>
                <label>"Zoom beim Abfragen (1 = ganzer Raum bis 4)"<input type="text" inputmode="decimal" bind:value=zoom /></label>
                <label>"Antwortart"<select bind:value=kind>
                    <option value="recall">"Recall (Selbstbewertung)"</option><option value="guess">"Guess (Eingabe)"</option><option value="quiz">"Quiz"</option>
                </select></label>
                <Show when=move || kind.get() == "guess"><label>"Weitere gültige Antworten (eine pro Zeile)"<textarea bind:value=accept /></label></Show>
                <Show when=move || kind.get() == "quiz"><label>"Quizoptionen (eine pro Zeile: + richtig, - falsch)"<textarea rows="5" bind:value=quiz /></label></Show>
            </fieldset>
            <div class="tabs">
                <button type="button" disabled=move || busy.get() || dragging.get() on:click=move |_| {
                    let Some(l) = cancel.selected_locus() else { return; };
                    if let Err(error) = persist_draft(&storage_key.get_value(), None) {
                        storage_error.set(error); return;
                    }
                    recovery.set(None); storage_error.set(String::new());
                    let draft = Draft::from_locus(&l);
                    last_attempted.set_value(draft.clone());
                    last_persisted.set_value(draft.clone());
                    place.set(draft.place); item.set(draft.item); note.set(draft.note); image.set(draft.image); size.set(draft.size); zoom.set(draft.zoom);
                    kind.set(draft.kind); accept.set(draft.accept); quiz.set(draft.quiz);
                    dirty.set(false); confirm_delete.set(false); cancel.message.set(String::new());
                }>"Änderungen verwerfen"</button>
                <button type="button" disabled=move || busy.get() || dragging.get() on:click=move |_| revealed.set(!revealed.get())>{move || if revealed.get() { "Bild verbergen" } else { "Bild aufdecken" }}</button>
                <button type="button" disabled=move || busy.get() || dirty.get() || dragging.get() on:click=move |_| {
                    if confirm_delete.get() { delete.change(EditorAction::Delete { id: id.get_value() }); }
                    else { confirm_delete.set(true); }
                }>{move || if confirm_delete.get() { "Endgültig löschen?" } else { "Ort löschen" }}</button>
            </div>
            <Show when=move || dirty.get()><p>"Noch nicht gespeichert – ungültige Eingaben korrigieren oder verwerfen, bevor du den Ort wechselst."</p></Show>
        </form>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_steps_stay_inside_the_video() {
        assert_eq!(
            frame_step_time(1.0, true, Some(10.0)),
            1.0 + VIDEO_FRAME_SECONDS
        );
        assert_eq!(
            frame_step_time(1.0, false, Some(10.0)),
            1.0 - VIDEO_FRAME_SECONDS
        );
        assert_eq!(frame_step_time(0.0, false, Some(10.0)), 0.0);
        assert_eq!(frame_step_time(9.99, true, Some(10.0)), 10.0);
        assert_eq!(frame_step_time(2.0, true, None), 2.0 + VIDEO_FRAME_SECONDS);
        assert_eq!(
            frame_step_time(2.0, true, Some(f64::NAN)),
            2.0 + VIDEO_FRAME_SECONDS
        );
    }

    fn move_to(x: f64) -> EditorAction {
        EditorAction::Move {
            id: "locus".into(),
            x,
            y: 0.5,
        }
    }

    #[test]
    fn history_moves_entries_only_after_acknowledgement_and_branches() {
        let mut history = History::default();
        history.record(move_to(0.1), move_to(0.2));
        history.record(move_to(0.2), move_to(0.3));
        // A failed IPC does not call completed, so the retry remains available.
        assert_eq!(history.undo.last().unwrap().0, move_to(0.2));
        assert_eq!(history.undo.len(), 2);
        history.completed(false);
        assert_eq!(history.redo.last().unwrap().1, move_to(0.3));
        history.completed(true);
        assert_eq!(history.undo.len(), 2);
        assert!(history.redo.is_empty());
        history.completed(false);
        history.record(move_to(0.2), move_to(0.4));
        assert!(history.redo.is_empty());
        assert_eq!(history.undo.last().unwrap().1, move_to(0.4));
        history.record(move_to(0.4), move_to(0.4));
        assert_eq!(history.undo.len(), 2);
        for i in 0..150 {
            history.record(move_to(i as f64), move_to(i as f64 + 1.0));
        }
        assert_eq!(history.undo.len(), 100);
    }

    #[test]
    fn inverse_patches_preserve_content_and_refuse_structural_history() {
        let locus = LocusView {
            id: "locus".into(),
            place: "Herd".into(),
            item: "Merkur".into(),
            note: "Notiz".into(),
            image: Some("hash".into()),
            image_size: 0.25,
            zoom: 2.0,
            bound: false,
            bound_fields: vec![],
            time: None,
            number: 1,
            x: 0.1,
            y: 0.5,
            answer: EditorAnswer::default(),
            picture: None,
            status: locora_app_api::Status::Neu,
        };
        let room = locora_app_api::RoomView {
            id: "room".into(),
            name: "Küche".into(),
            video: false,
            backdrop: None,
            loci: vec![locus.clone()],
        };
        assert_eq!(inverse(&move_to(0.8), &room), Some(move_to(0.1)));
        let resize = EditorAction::Resize {
            id: "locus".into(),
            image_size: 0.8,
        };
        assert_eq!(
            inverse(&resize, &room),
            Some(EditorAction::Resize {
                id: "locus".into(),
                image_size: 0.25
            })
        );
        let save = EditorAction::Save {
            id: locus.id.clone(),
            content: Draft::from_locus(&locus).content().unwrap(),
        };
        assert_eq!(inverse(&save, &room), Some(save));
        assert!(inverse(&EditorAction::Delete { id: locus.id }, &room).is_none());
        assert!(inverse(&EditorAction::Create { x: 0.2, y: 0.3 }, &room).is_none());
    }

    #[test]
    fn draft_storage_keys_are_versioned_scoped_and_unambiguous() {
        assert_ne!(draft_key("a", "bc", "d"), draft_key("ab", "c", "d"));
        assert_ne!(draft_key("a", "r", "l"), draft_key("b", "r", "l"));
        assert_ne!(draft_key("a", "r", "l"), draft_key("a", "s", "l"));
        assert_ne!(draft_key("a:b", "c", "d"), draft_key("a", "b:c", "d"));
        assert!(draft_key("p", "r", "l").starts_with("locora.editor.draft.v1:"));
    }

    #[test]
    fn lists_trim_ignore_empty_lines_and_reject_limits() {
        assert_eq!(
            parse_list(" Merkur\r\n\r\n Venus \n Erde ").unwrap(),
            ["Merkur", "Venus", "Erde"]
        );
        assert_eq!(parse_list("Apfel\nApfel").unwrap().len(), 2);
        for text in [
            "".into(),
            " \n ".into(),
            "x".repeat(501),
            "x\n".repeat(101),
            " ".repeat(65537),
        ] {
            assert!(parse_list(&text).is_err());
        }
    }

    #[test]
    fn paste_routes_are_inset_distinct_and_serpentine() {
        use locora_app_api::paste_route_position as point;
        assert_eq!(point(0, 1), (0.5, 0.5));
        assert_eq!(point(3, 10).0, point(4, 10).0); // row turn keeps its x
        for count in 1..=100 {
            let points = (0..count).map(|i| point(i, count)).collect::<Vec<_>>();
            for (i, &(x, y)) in points.iter().enumerate() {
                assert!((0.09..=0.91).contains(&x) && (0.09..=0.91).contains(&y));
                assert!(!points[..i].contains(&(x, y)));
            }
        }
        assert!(point(4, 10).0 > point(5, 10).0);
    }

    #[test]
    fn committed_resize_updates_only_an_unmodified_size_draft() {
        assert_eq!(synchronized_size("0.25", 0.25, 0.4).as_deref(), Some("0.4"));
        assert_eq!(synchronized_size("0.7", 0.25, 0.4), None);
        assert_eq!(synchronized_size("", 0.25, 0.4), None);
    }

    #[test]
    fn incomplete_drafts_are_not_sent_as_valid_content() {
        let mut draft = Draft {
            place: "Herd".into(),
            item: "Merkur".into(),
            note: String::new(),
            image: String::new(),
            size: "0.25".into(),
            zoom: "1".into(),
            kind: "recall".into(),
            accept: String::new(),
            quiz: String::new(),
        };
        assert!(draft.content().is_ok());
        let committed = draft.clone();
        assert!(recoverable_draft(None, &committed).is_none());
        assert!(recoverable_draft(Some(committed.clone()), &committed).is_none());
        // Recovery compares raw text, not parsed content: invalid and partially
        // typed values must survive rather than being normalized or rejected.
        draft.size.clear();
        draft.quiz = "+ angefangene Option".into();
        assert_eq!(
            recoverable_draft(Some(draft.clone()), &committed),
            Some(draft.clone())
        );
        for value in ["", "NaN", "inf", "0", "1.01"] {
            draft.size = value.into();
            assert!(draft.content().is_err());
        }
        draft.size = "0.4".into();
        for value in ["0.5", "4.5", "nah"] {
            draft.zoom = value.into();
            assert!(draft.content().is_err(), "{value}");
        }
        draft.zoom = "2,5".into();
        assert_eq!(draft.content().unwrap().zoom, 2.5);
        draft.kind = "quiz".into();
        draft.quiz = "+ Merkur".into();
        assert!(draft.content().is_err());
        draft.quiz.push_str("\n- Venus");
        let content = draft.content().unwrap();
        assert_eq!(content.answer.kind, EditorAnswerKind::Quiz);
        assert_eq!(content.image_size, 0.4);
        assert_eq!(draft.quiz, "+ Merkur\n- Venus");
        draft.place.clear();
        assert!(draft.content().is_err());
    }

    #[test]
    fn moving_keeps_the_initial_grab_offset_and_clamps() {
        assert_eq!(moved_position((0.4, 0.3), (0.1, 0.2)), (0.5, 0.5));
        assert_eq!(moved_position((0.4, 0.3), (-2.0, 2.0)), (0.0, 1.0));
    }

    #[test]
    fn coordinates_use_stage_bounds_and_clamp_edges() {
        assert_eq!(
            stage_point(350.0, 200.0, 100.0, 50.0, 500.0, 300.0),
            Some((0.5, 0.5))
        );
        assert_eq!(
            stage_point(-20.0, 800.0, 100.0, 50.0, 500.0, 300.0),
            Some((0.0, 1.0))
        );
        assert_eq!(stage_point(1.0, 1.0, 0.0, 0.0, 0.0, 5.0), None);
        assert_eq!(stage_point(f64::NAN, 1.0, 0.0, 0.0, 5.0, 5.0), None);
    }
    #[test]
    fn resizing_is_centered_and_clamped() {
        assert_eq!(resized(0.25, 50.0, 1000.0), 0.35);
        assert_eq!(resized(0.25, -900.0, 1000.0), 0.05);
        assert_eq!(resized(0.25, 900.0, 1000.0), 1.0);
    }
    #[test]
    fn answer_notation_parses_without_stale_fields() {
        assert_eq!(
            parse_answer("recall", "old", "+ old").unwrap(),
            EditorAnswer::default()
        );
        let guess = parse_answer("guess", " Mercury \n\n Merkur ", "+ old").unwrap();
        assert_eq!(guess.accept, ["Mercury", "Merkur"]);
        assert!(guess.options.is_empty());
        let quiz = parse_answer("quiz", "old", "+ Merkur\n- Venus\n- +Plus").unwrap();
        assert!(quiz.accept.is_empty());
        assert_eq!(quiz.options[2].text, "+Plus");
        assert!(quiz.options[0].correct);
        assert!(!quiz.options[1].correct);
        assert!(parse_answer("quiz", "", "ohne Vorzeichen").is_err());
    }
}
