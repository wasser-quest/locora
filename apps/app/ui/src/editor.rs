//! Room editor and pointer gestures; mutations and validation go through IPC.
use leptos::{html, prelude::*, task::spawn_local};
use locora_app_api::{
    CMD_EDITOR_CHANGE, EditorAction, EditorAnswer, EditorAnswerKind, EditorChange, EditorUpdated,
    LocusContent, LocusView, PalaceView, QuizOption,
};
use serde::Serialize;
use web_sys::PointerEvent;

use crate::{
    Answer,
    sply::{Shown, Sply},
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
#[derive(Clone, PartialEq)]
struct Draft {
    place: String,
    item: String,
    note: String,
    image: String,
    size: String,
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
        let answer = parse_answer(&self.kind, &self.accept, &self.quiz)?;
        answer.validate().map_err(str::to_owned)?;
        Ok(LocusContent {
            place: self.place.clone(),
            item: self.item.clone(),
            note: self.note.clone(),
            image: (!self.image.trim().is_empty()).then(|| self.image.trim().to_owned()),
            image_size,
            answer,
        })
    }
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
        if self.busy.get_untracked() {
            return;
        }
        let data = self.data.get_untracked();
        let Some(room) = data.rooms.get(self.room.get_untracked()) else {
            return;
        };
        let pasted = matches!(&action, EditorAction::Paste { .. });
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
                    if pasted {
                        this.paste_text.set(String::new());
                    }
                    this.selected.set(result.selected);
                    this.dirty.set(false);
                    this.data.set(result.palace);
                    this.message.set("Gespeichert".into());
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
    };
    let editing = RwSignal::new(initial_editing);
    let adding = RwSignal::new(false);
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
    view! {
        <div class="modes">
            <label><input type="checkbox" prop:checked=move || editing.get()
                disabled=move || busy.get() || dirty.get() || dragging.get()
                on:change=move |ev| { editing.set(event_target_checked(&ev)); adding.set(false); } />"Bearbeiten"</label>
            <Show when=move || editing.get()>
                <label><input type="checkbox" bind:checked=adding disabled=move || busy.get() || dirty.get() || dragging.get() />"Ort setzen (Bühne anklicken)"</label>
                <button disabled=move || busy.get() || dirty.get() || dragging.get()
                    on:click={let editor = editor.clone(); move |_| {
                        revealed.set(false);
                        editor.change(EditorAction::Create { x: 0.5, y: 0.5 });
                    }}>"Ort in der Mitte hinzufügen"</button>
                <span>"Marker ziehen (Tastatur: Pfeiltasten) · Änderungen speichern automatisch"</span>
            </Show>
        </div>
        <Show when=move || editing.get()>
            <ListPaste editor=paste.clone() />
        </Show>
        <div class="tabs" style:margin-top="1rem">
            {move || {
                let tabs = tabs.clone();
                tabs.data.get().rooms.iter().enumerate().map(|(i, room)| {
                    let tabs = tabs.clone();
                    view! {
                        <button class:on=move || tabs.room.get() == i disabled=move || busy.get() || dirty.get() || dragging.get()
                            on:click=move |_| { tabs.room.set(i); tabs.selected.set(None); revealed.set(false); adding.set(false); tabs.message.set(String::new()); }>
                            <crate::RoomIcon backdrop=room.backdrop.clone() />
                            {room.name.clone()}
                        </button>
                    }
                }).collect_view()
            }}
        </div>
        {move || {
            // Only rebuild the stage on room changes / committed mutations, never on pointer moves.
            let stage = stage.clone();
            let data = stage.data.get();
            let Some(room) = data.rooms.get(stage.room.get()).cloned() else {
                return view! { <p class="empty">"Dieser Palast hat noch keine Räume."</p> }.into_any();
            };
            view! { <RoomStage editor=stage room=room marker=data.marker editing=editing adding=adding revealed=revealed /> }.into_any()
        }}
        <p role="status" class="notes">{move || message.get()}</p>
        {move || {
            let panel = panel.clone();
            if editing.get() {
                let items = panel.clone();
                view! { <For
                    each={move || items.selected_locus().into_iter().collect::<Vec<_>>()}
                    key=|l| l.id.clone()
                    children={move |l| view! { <ContentForm locus=l editor=panel.clone() revealed=revealed /> }}
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
        <section class="card panel editor-form">
            <label>"Liste einfügen (Strg+V hier): ein Begriff pro Zeile"
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
        </section>
    }
}

#[component]
fn RoomStage(
    editor: Editor,
    room: locora_app_api::RoomView,
    marker: locora_app_api::Media,
    editing: RwSignal<bool>,
    adding: RwSignal<bool>,
    revealed: RwSignal<bool>,
) -> impl IntoView {
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
    view! {
        <Show when=move || editing.get() && video><p class="notes">"Video: Inhalte und Positionen bearbeitbar; neue Orte mit Zeit vorerst über die CLI."</p></Show>
        <div class="stage" class:editing=move || editing.get() node_ref=node tabindex="0"
            aria-label="Raum – im Bearbeitungsmodus mit Strg+V eine Liste als Route einfügen"
            on:paste=move |ev: web_sys::ClipboardEvent| {
                // Text fields are outside the stage, so ordinary text paste
                // never reaches this handler. Do not request clipboard access.
                if !editing.get_untracked() || video || busy.get_untracked() || dirty.get_untracked() || gesture.with_untracked(|g| g.is_some()) {
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
                if editing.get() && adding.get() && !video { begin.with_value(|begin| begin(ev, DragKind::Create)); }
            }
            on:pointermove=move |ev| { if !busy.get_untracked() { update(&ev); } }
            on:pointerup=move |ev| {
                if busy.get_untracked() { return; }
                update(&ev);
                let Some(g) = gesture.get_untracked().filter(|g| g.pointer == ev.pointer_id()) else { return; };
                if let Some(stage) = node.get() { let _ = stage.release_pointer_capture(g.pointer); }
                let action = match g.kind {
                    DragKind::Create if !g.moved => { adding.set(false); revealed.set(false); Some(EditorAction::Create { x: g.point.0, y: g.point.1 }) }
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
            {room.backdrop.map(|media| view! { <Shown media=media class="backdrop" /> })}
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
                let l = StoredValue::new(locus.clone());
                let marker = marker.clone();
                view! {
                    <div class="marker"
                        style:left=move || format!("{}%", preview_position(&l.get_value(), gesture.get().as_ref()).0 * 100.0)
                        style:top=move || format!("{}%", preview_position(&l.get_value(), gesture.get().as_ref()).1 * 100.0)
                    >
                        <Sply media=marker state=Signal::stored(locus.status.event()) />
                        <button title=format!("Ort {}: {}", locus.number, locus.place)
                            disabled=move || busy.get() || dirty.get()
                            on:pointerdown=move |ev: PointerEvent| {
                                ev.stop_propagation();
                                if editing.get() { begin.with_value(|begin| begin(ev, DragKind::Move(l.get_value()))); }
                            }
                            on:keydown=move |ev: web_sys::KeyboardEvent| {
                                if !editing.get() || busy.get_untracked() || dirty.get_untracked() { return; }
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
                        <span class="num">{locus.number}</span>
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
fn ContentForm(locus: LocusView, editor: Editor, revealed: RwSignal<bool>) -> impl IntoView {
    let place = RwSignal::new(locus.place.clone());
    let item = RwSignal::new(locus.item.clone());
    let note = RwSignal::new(locus.note.clone());
    let image = RwSignal::new(locus.image.clone().unwrap_or_default());
    let size = RwSignal::new(locus.image_size.to_string());
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
    let dirty = editor.dirty;
    let busy = editor.busy;
    let confirm_delete = RwSignal::new(false);
    let gesture = editor.gesture;
    let dragging = Memo::new(move |_| gesture.with(|g| g.is_some()));
    let save = editor.clone();
    let last_attempted = StoredValue::new(Draft::from_locus(&locus));
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
            kind: kind.get(),
            accept: accept.get(),
            quiz: quiz.get(),
        };
        // Serialize saves, but keep the draft editable during IPC. Once the
        // current write finishes this effect sends the newest draft, not an
        // older queued keystroke. A rejected revision is not retried forever.
        if busy.get()
            || dragging.get()
            || locus.bound
            || last_attempted.with_value(|old| old == &draft)
        {
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
    let picking = RwSignal::new(false);
    let picker = editor.clone();
    let pick_id = locus.id.clone();
    let pick = move |_| {
        if busy.get_untracked() || dirty.get_untracked() || dragging.get_untracked() {
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
        let this = picker.clone();
        picking.set(true);
        busy.set(true);
        this.message.set(String::new());
        spawn_local(async move {
            #[derive(Serialize)]
            struct Args {
                target: locora_app_api::PictureTarget,
            }
            match call::<Option<EditorUpdated>>(
                locora_app_api::CMD_EDITOR_PICK_PICTURE,
                Args { target },
            )
            .await
            {
                Ok(Some(result)) => {
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
    };
    let delete = editor.clone();
    let cancel = editor.clone();
    let id = StoredValue::new(locus.id.clone());
    view! {
        <form class="card panel editor-form" on:submit=move |ev| ev.prevent_default()>
            <div class="progress">{format!("Ort {} bearbeiten", locus.number)}</div>
            <Show when=move || locus.bound><p class="notes">"Workbook-Ort: Inhalte bitte in der Quelldatei ändern. Verschieben und Bildgröße ziehen sind möglich."</p></Show>
            <fieldset disabled=move || picking.get() || dragging.get() || locus.bound
                on:input=move |_| confirm_delete.set(false)
                on:change=move |_| confirm_delete.set(false)>
                <label>"Ort"<input type="text" bind:value=place /></label>
                <label>"Inhalt"<input type="text" bind:value=item /></label>
                <label>"Merkhilfe"<textarea bind:value=note /></label>
                <div class="tabs">
                    <button type="button" disabled=move || busy.get() || dirty.get() || dragging.get() on:click=pick>"Bild wählen…"</button>
                    <button type="button" disabled=move || busy.get() || dirty.get() || dragging.get() || image.get().is_empty()
                        on:click=move |_| image.set(String::new())>"Bild entfernen"</button>
                </div>
                <label>"Bildgröße (Anteil der Raumbreite, 0.05–1)"<input type="number" min="0.05" max="1" step="any" bind:value=size /></label>
                <label>"Antwortart"<select bind:value=kind>
                    <option value="recall">"Recall (Selbstbewertung)"</option><option value="guess">"Guess (Eingabe)"</option><option value="quiz">"Quiz"</option>
                </select></label>
                <Show when=move || kind.get() == "guess"><label>"Weitere gültige Antworten (eine pro Zeile)"<textarea bind:value=accept /></label></Show>
                <Show when=move || kind.get() == "quiz"><label>"Quizoptionen (eine pro Zeile: + richtig, - falsch)"<textarea rows="5" bind:value=quiz /></label></Show>
            </fieldset>
            <div class="tabs">
                <button type="button" disabled=move || busy.get() || dragging.get() on:click=move |_| {
                    let Some(l) = cancel.selected_locus() else { return; };
                    let draft = Draft::from_locus(&l);
                    last_attempted.set_value(draft.clone());
                    place.set(draft.place); item.set(draft.item); note.set(draft.note); image.set(draft.image); size.set(draft.size);
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
            kind: "recall".into(),
            accept: String::new(),
            quiz: String::new(),
        };
        assert!(draft.content().is_ok());
        for value in ["", "NaN", "inf", "0", "1.01"] {
            draft.size = value.into();
            assert!(draft.content().is_err());
        }
        draft.size = "0.4".into();
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
