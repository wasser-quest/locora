//! A journey: one question at a time. Recall: Space reveals, 1–4 grade.
//! Guess: type and Enter (empty Enter reveals). Quiz: tick and confirm.
//! Revealing shows the locus picture, because it belongs to the answer.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    ActionDone, AnswerKind, CMD_JOURNEY_ACTION, CMD_JOURNEY_ANSWER, CMD_JOURNEY_INPUTS,
    CMD_JOURNEY_REVEAL, CMD_JOURNEY_START, Feedback, InputField, InputValue, JourneyStart,
    Question, Reply, Revealed, Step, Summary,
};
use serde::Serialize;

use crate::sply::{Shown, Sply};
use crate::tauri::{NoArgs, call};
use crate::{Answer, Screen};

#[derive(Serialize)]
struct StartArgs {
    start: JourneyStart,
}

#[derive(Serialize)]
struct ReplyArgs {
    reply: Reply,
}

#[derive(Serialize)]
struct ActionArgs {
    key: String,
    revealed: bool,
}

/// Where the current question stands.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Asking,
    /// Recall: answer visible, waiting for the self-rating.
    Revealed(Revealed),
    /// Guess/Quiz (or after rating): verdict and answer, then on to `next`.
    Judged(Box<Feedback>),
}

impl Phase {
    /// Only revealed answers may supply a picture for the room stage.
    fn stage_answer(&self) -> Option<&Revealed> {
        match self {
            Self::Asking => None,
            Self::Revealed(r) => Some(r),
            Self::Judged(f) => Some(&f.revealed),
        }
    }
}

#[component]
pub fn JourneyView(start: JourneyStart, screen: RwSignal<Screen>) -> impl IntoView {
    let question = RwSignal::new(None::<Question>);
    let phase = RwSignal::new(Phase::Asking);
    let summary = RwSignal::new(None::<Summary>);
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    // Workbook inputs are asked first; without any, the journey starts at once.
    let fields = RwSignal::new(None::<Vec<InputField>>);
    let start = StoredValue::new(start);
    let begin = move |inputs: Vec<InputValue>| {
        // On failure (e.g. text in a number input) the form comes back with
        // what was typed, so it can be corrected.
        let retry = fields.get_untracked().map(|list| {
            list.into_iter()
                .map(|mut field| {
                    if let Some(typed) = inputs
                        .iter()
                        .find(|v| v.palace_id == field.palace_id && v.key == field.key)
                    {
                        field.default = typed.value.clone();
                    }
                    field
                })
                .collect::<Vec<_>>()
        });
        let start = JourneyStart {
            inputs,
            ..start.get_value()
        };
        fields.set(None);
        error.set(None);
        spawn_local(async move {
            match call::<Step>(CMD_JOURNEY_START, StartArgs { start }).await {
                Ok(Step::Question(q)) => question.set(Some(q)),
                Ok(Step::Done(s)) => summary.set(Some(s)),
                Err(e) => {
                    fields.set(retry);
                    error.set(Some(e));
                }
            }
        });
    };
    spawn_local(async move {
        let args = StartArgs {
            start: start.get_value(),
        };
        match call::<Vec<InputField>>(CMD_JOURNEY_INPUTS, args).await {
            Ok(list) if list.is_empty() => begin(Vec::new()),
            Ok(list) => fields.set(Some(list)),
            Err(e) => error.set(Some(e)),
        }
    });

    let reveal = move || {
        if busy.get_untracked() || phase.get_untracked() != Phase::Asking {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<Revealed>(CMD_JOURNEY_REVEAL, NoArgs {}).await {
                Ok(r) => phase.set(Phase::Revealed(r)),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };
    let reply = move |reply: Reply| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            match call::<Feedback>(CMD_JOURNEY_ANSWER, ReplyArgs { reply }).await {
                Ok(f) => phase.set(Phase::Judged(Box::new(f))),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };
    // A workbook action on the current question; „reveal“ actions show the answer.
    let act = move |key: String| {
        if busy.get_untracked() {
            return;
        }
        let revealed = matches!(phase.get_untracked(), Phase::Revealed(_));
        busy.set(true);
        spawn_local(async move {
            match call::<ActionDone>(CMD_JOURNEY_ACTION, ActionArgs { key, revealed }).await {
                Ok(done) => {
                    let kind = done.question.kind;
                    question.set(Some(done.question));
                    busy.set(false);
                    match (done.revealed, done.reveal || revealed) {
                        (Some(r), true) if kind == AnswerKind::Recall => {
                            phase.set(Phase::Revealed(r))
                        }
                        (_, true) => reply(Reply::Forfeit),
                        _ => {}
                    }
                }
                Err(e) => {
                    error.set(Some(e));
                    busy.set(false);
                }
            }
        });
    };
    let next = move || {
        let Phase::Judged(feedback) = phase.get_untracked() else {
            return;
        };
        match feedback.next {
            Step::Question(q) => {
                question.set(Some(q));
                phase.set(Phase::Asking);
            }
            Step::Done(s) => summary.set(Some(s)),
        }
    };

    // Keyboard: Space/Enter reveal or continue, 1–4 rate a recall.
    let keys = window_event_listener(ev::keydown, move |e| {
        let typing = e
            .target()
            .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlInputElement>(t).ok())
            .is_some_and(|i| i.type_() == "text");
        if typing || summary.get_untracked().is_some() {
            return;
        }
        let recall = question
            .get_untracked()
            .is_some_and(|q| q.kind == AnswerKind::Recall);
        match (phase.get_untracked(), e.key().as_str()) {
            (Phase::Asking, " ") if recall => {
                e.prevent_default();
                reveal();
            }
            (Phase::Revealed(_), k @ ("1" | "2" | "3" | "4")) => {
                reply(Reply::Recall(k.parse().unwrap_or(3)));
            }
            (Phase::Revealed(_), " " | "Enter") => {
                e.prevent_default();
                reply(Reply::Recall(3));
            }
            (Phase::Judged(_), " " | "Enter") => {
                e.prevent_default();
                next();
            }
            _ => {}
        }
    });
    on_cleanup(move || keys.remove());

    view! {
        <header class="bar">
            <button class="back" title="Beenden" on:click=move |_| screen.set(Screen::Home)>
                "←"
            </button>
            <h1>
                {move || question.get().map(|q| q.palace).unwrap_or_else(|| "Journey".into())}
            </h1>
            <span class="progress">
                {move || {
                    if summary.get().is_some() {
                        String::new()
                    } else {
                        question.get().map(|q| format!("{}/{}", q.n, q.total)).unwrap_or_default()
                    }
                }}
            </span>
        </header>
        {move || error.get().map(|e| view! { <p class="notes">{e}</p> })}
        {move || {
            if let Some(s) = summary.get() {
                return view! { <Done summary=s screen=screen /> }.into_any();
            }
            if let Some(list) = fields.get() {
                return view! { <Inputs fields=list begin=begin /> }.into_any();
            }
            let Some(q) = question.get() else {
                return view! { <p class="loading">"Lädt …"</p> }.into_any();
            };
            view! {
                <Stage question=q.clone() phase=phase />
                <section class="card panel">
                    <div class="progress">{q.room.clone()}</div>
                    <div class="prompt">{q.prompt.clone()}</div>
                    {q
                        .picture
                        .clone()
                        .map(|m| {
                            view! {
                                <div class="picture">
                                    <Shown media=m />
                                </div>
                            }
                        })}
                    {(!q.actions.is_empty())
                        .then(|| {
                            let actions = q.actions.clone();
                            view! {
                                <Show when=move || {
                                    !matches!(phase.get(), Phase::Judged(_))
                                }>
                                    <div class="actions">
                                        {actions
                                            .iter()
                                            .cloned()
                                            .map(|key| {
                                                let label = key.clone();
                                                view! {
                                                    <button
                                                        disabled=move || busy.get()
                                                        on:click=move |_| act(key.clone())
                                                    >
                                                        {label}
                                                    </button>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                </Show>
                            }
                        })}
                    {move || match phase.get() {
                        Phase::Asking => {
                            view! { <Ask question=q.clone() busy=busy reveal=reveal reply=reply /> }
                                .into_any()
                        }
                        Phase::Revealed(r) => {
                            view! {
                                <Answer solution=r.solution note=r.note picture=r.picture />
                                <Grades reply=reply busy=busy />
                            }
                                .into_any()
                        }
                        Phase::Judged(f) => {
                            view! { <Judged feedback=*f question=q.clone() next=next /> }.into_any()
                        }
                    }}
                </section>
            }
                .into_any()
        }}
    }
}

/// The room with the asked place marked (state „fällig“).
#[component]
fn Stage(question: Question, phase: RwSignal<Phase>) -> impl IntoView {
    view! {
        <div class="stage">
            {question.backdrop.clone().map(|m| view! { <Shown media=m class="backdrop" /> })}
            {move || {
                phase.get().stage_answer().cloned().and_then(|r| r.picture.map(|m| view! {
                    <crate::LocusPicture media=m x=question.x y=question.y size=r.image_size />
                }))
            }}
            <div
                class="marker"
                style:left=format!("{}%", question.x * 100.0)
                style:top=format!("{}%", question.y * 100.0)
            >
                <Sply media=question.marker.clone() state=Signal::stored("faellig") />
            </div>
        </div>
    }
}

#[component]
fn Ask(
    question: Question,
    busy: RwSignal<bool>,
    reveal: impl Fn() + Copy + Send + Sync + 'static,
    reply: impl Fn(Reply) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    match question.kind {
        AnswerKind::Recall => view! {
            <button class="primary" disabled=move || busy.get() on:click=move |_| reveal()>
                "Aufdecken (Leertaste)"
            </button>
        }
        .into_any(),
        AnswerKind::Guess => {
            let text = RwSignal::new(String::new());
            let submit = move || {
                let t = text.get_untracked();
                reply(if t.trim().is_empty() {
                    Reply::Forfeit
                } else {
                    Reply::Guess(t)
                });
            };
            view! {
                <form on:submit=move |e| {
                    e.prevent_default();
                    submit();
                }>
                    <input
                        type="text"
                        placeholder="Antwort eingeben – Enter (leer = aufdecken)"
                        autofocus=true
                        bind:value=text
                    />
                </form>
                <button class="primary" disabled=move || busy.get() on:click=move |_| submit()>
                    "Prüfen"
                </button>
            }
            .into_any()
        }
        AnswerKind::Quiz => {
            let picked = RwSignal::new(Vec::<u32>::new());
            view! {
                <div class="options">
                    {question
                        .options
                        .iter()
                        .enumerate()
                        .map(|(i, text)| {
                            let i = i as u32;
                            view! {
                                <label>
                                    <input
                                        type="checkbox"
                                        prop:checked=move || picked.get().contains(&i)
                                        on:change=move |_| {
                                            picked
                                                .update(|p| {
                                                    if let Some(at) = p.iter().position(|x| *x == i) {
                                                        p.remove(at);
                                                    } else {
                                                        p.push(i);
                                                    }
                                                })
                                        }
                                    />
                                    {text.clone()}
                                </label>
                            }
                        })
                        .collect_view()}
                </div>
                <div class="progress">"Alle richtigen wählen."</div>
                <button
                    class="primary"
                    disabled=move || busy.get() || picked.get().is_empty()
                    on:click=move |_| reply(Reply::Quiz(picked.get_untracked()))
                >
                    "Prüfen"
                </button>
                <button on:click=move |_| reply(Reply::Forfeit)>"Aufdecken"</button>
            }
                .into_any()
        }
    }
}

#[component]
fn Grades(reply: impl Fn(Reply) + Copy + 'static, busy: RwSignal<bool>) -> impl IntoView {
    let grades = [(1u8, "Nochmal"), (2, "Schwer"), (3, "Gut"), (4, "Leicht")];
    view! {
        <div class="grades">
            {grades
                .into_iter()
                .map(|(g, label)| {
                    view! {
                        <button disabled=move || busy.get() on:click=move |_| reply(Reply::Recall(g))>
                            {label}
                            <small>{g}</small>
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[component]
fn Judged(feedback: Feedback, question: Question, next: impl Fn() + 'static) -> impl IntoView {
    let r = feedback.revealed;
    let automatic = question.kind != AnswerKind::Recall;
    let right: Vec<String> = r
        .right
        .iter()
        .filter_map(|i| question.options.get(*i as usize).cloned())
        .collect();
    view! {
        {automatic
            .then(|| {
                if feedback.correct {
                    view! { <div class="verdict ok">"Richtig"</div> }.into_any()
                } else {
                    view! { <div class="verdict again">"Nochmal"</div> }.into_any()
                }
            })}
        <Answer solution=r.solution note=r.note picture=r.picture />
        {(!right.is_empty())
            .then(|| {
                view! { <div class="progress">{format!("Richtig: {}", right.join(", "))}</div> }
            })}
        <button class="primary" on:click=move |_| next()>
            "Weiter (Leertaste)"
        </button>
    }
}

#[component]
fn Done(summary: Summary, screen: RwSignal<Screen>) -> impl IntoView {
    view! {
        <section class="card panel">
            {if summary.asked == 0 {
                view! { <div class="prompt">"Nichts zu üben – alles erledigt."</div> }.into_any()
            } else {
                view! {
                    <div class="prompt">
                        {format!(
                            "{} von {} gewusst · {} Punkte",
                            summary.correct,
                            summary.asked,
                            summary.points,
                        )}
                    </div>
                }
                    .into_any()
            }}
            {summary
                .notes
                .into_iter()
                .map(|n| view! { <p class="notes">{n}</p> })
                .collect_view()}
            <button class="primary" on:click=move |_| screen.set(Screen::Home)>
                "Zur Übersicht"
            </button>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_app_api::{Media, MediaKind};

    #[test]
    fn stage_answer_stays_hidden_until_revealed_and_resets_for_next_question() {
        let answer = Revealed {
            image_size: 0.4,
            solution: "Sonne".into(),
            note: String::new(),
            picture: Some(Media {
                hash: "bild".into(),
                kind: MediaKind::Image,
            }),
            right: Vec::new(),
        };
        assert!(Phase::Asking.stage_answer().is_none());
        let revealed = Phase::Revealed(answer.clone());
        assert_eq!(revealed.stage_answer(), Some(&answer));
        let judged = Phase::Judged(Box::new(Feedback {
            correct: true,
            points: 1,
            revealed: answer.clone(),
            next: Step::Done(Summary::default()),
        }));
        assert_eq!(judged.stage_answer(), Some(&answer));
        let next = Phase::Asking;
        assert!(next.stage_answer().is_none());
        let reverse = Phase::Revealed(Revealed {
            picture: None,
            ..answer
        });
        assert!(reverse.stage_answer().unwrap().picture.is_none());
    }
}

/// The palace's workbook inputs, prefilled with their defaults.
#[component]
fn Inputs(
    fields: Vec<InputField>,
    begin: impl Fn(Vec<InputValue>) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let values: Vec<RwSignal<String>> = fields
        .iter()
        .map(|f| RwSignal::new(f.default.clone()))
        .collect();
    let several = fields.iter().any(|f| f.palace_id != fields[0].palace_id);
    let collected = StoredValue::new(
        fields
            .iter()
            .cloned()
            .zip(values.clone())
            .collect::<Vec<_>>(),
    );
    let submit = move || {
        begin(collected.with_value(|list| {
            list.iter()
                .map(|(f, v)| InputValue {
                    palace_id: f.palace_id.clone(),
                    key: f.key.clone(),
                    value: v.get_untracked(),
                })
                .collect()
        }))
    };
    view! {
        <form
            class="card panel"
            on:submit=move |e| {
                e.prevent_default();
                submit();
            }
        >
            <div class="prompt">"Eingaben für diese Journey"</div>
            {fields
                .into_iter()
                .zip(values)
                .map(|(f, value)| {
                    let label = if several {
                        format!("{} · {}", f.palace, f.key)
                    } else {
                        f.key.clone()
                    };
                    // Fields fit the input's type; values travel as the text
                    // the workbook parses (`true`/`false` for Ja/Nein).
                    match f.kind.as_str() {
                        "boolean" => view! {
                            <label class="input check">
                                <input
                                    type="checkbox"
                                    prop:checked=move || value.get() == "true"
                                    on:change=move |e| {
                                        value.set(event_target_checked(&e).to_string())
                                    }
                                />
                                <span>{label}</span>
                            </label>
                        }
                        .into_any(),
                        kind => {
                            let number = kind == "number";
                            view! {
                                <label class="input">
                                    <span>{label}</span>
                                    <input
                                        type=if number { "number" } else { "text" }
                                        step=number.then_some("any")
                                        inputmode=number.then_some("decimal")
                                        bind:value=value
                                    />
                                </label>
                            }
                            .into_any()
                        }
                    }
                })
                .collect_view()}
            <button class="primary" type="submit">"Los ▶"</button>
        </form>
    }
}
