//! A journey: one question at a time. Recall: Space reveals, 1–4 grade.
//! Guess: type and Enter (empty Enter reveals). Quiz: tick and confirm.
//! Revealing shows the locus picture, because it belongs to the answer.

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    ActionDone, AnswerKind, CMD_GAMIFICATION_STATUS, CMD_JOURNEY_ACTION, CMD_JOURNEY_ANSWER,
    CMD_JOURNEY_INPUTS, CMD_JOURNEY_REVEAL, CMD_JOURNEY_START, Feedback, GameView,
    GamificationStatus, InputField, InputValue, JourneyStart, Question, Reply, Revealed, Step,
    Summary, Trophy, UiAssets,
};
use serde::Serialize;

use crate::sply::{Shown, Sply, TimedCommand, VideoBackdrop, fit_stage};
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
    /// On the question just answered (its feedback is showing).
    after: bool,
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

#[derive(Clone, Copy)]
struct VideoProgress(StoredValue<Option<(String, f64)>>);

#[derive(Clone, Copy)]
struct VideoReady(RwSignal<bool>);

#[derive(Clone, Copy)]
struct VideoPosition(RwSignal<f64>);

fn refresh_game(status: RwSignal<Option<GamificationStatus>>) {
    spawn_local(async move {
        if let Ok(value) =
            call::<Option<GamificationStatus>>(CMD_GAMIFICATION_STATUS, NoArgs {}).await
        {
            status.set(value);
        }
    });
}

/// Where a reverse question was answered, as room fractions, and whether it was within the radius.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Guess {
    n: u32,
    x: f64,
    y: f64,
    hit: bool,
}

#[derive(Clone, Copy)]
struct GuessSpot(RwSignal<Option<Guess>>);

/// The zoom shift of `zoom_view`: where the stage moves the room for a locus at `target`.
fn zoom_shift(target: f64, zoom: f64) -> f64 {
    (0.5 - target * zoom).clamp(1.0 - zoom, 0.0)
}

/// Stage fraction (0–1 of the stage box) of a room fraction, as the zoomed stage shows it.
fn stage_fraction(room: f64, target: f64, zoom: f64) -> f64 {
    zoom_shift(target, zoom) + zoom * room
}

/// Room fraction under a stage fraction: the inverse of `stage_fraction`.
fn room_fraction(stage: f64, target: f64, zoom: f64) -> f64 {
    (stage - zoom_shift(target, zoom)) / zoom
}

#[component]
pub fn JourneyView(start: JourneyStart, screen: RwSignal<Screen>) -> impl IntoView {
    provide_context(VideoProgress(StoredValue::new(None)));
    provide_context(GuessSpot(RwSignal::new(None::<Guess>)));
    let video_ready = RwSignal::new(true);
    provide_context(VideoReady(video_ready));
    provide_context(VideoPosition(RwSignal::new(0.0)));
    let question = RwSignal::new(None::<Question>);
    // The question's step number: the stage (and its video) belongs to a step, not to
    // every answer or workbook action that sets the question again on the same step.
    let stage_step = Memo::new(move |_| question.with(|q| q.as_ref().map(|q| q.n)));
    let phase = RwSignal::new(Phase::Asking);
    let summary = RwSignal::new(None::<Summary>);
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let game = RwSignal::new(None::<GamificationStatus>);
    // Feedback animations (assets/ui); the journey works without them.
    let art = crate::art();

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
                Ok(Step::Question(q)) => {
                    question.set(Some(q));
                    refresh_game(game);
                }
                Ok(Step::Done(s)) => {
                    summary.set(Some(s));
                    refresh_game(game);
                }
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

    // Quick mode: continue straight to the next question after reveal or answer.
    let prefs = crate::locus_display::signal();
    let next = move || {
        // Not while an action on the answered question is still running.
        if busy.get_untracked() {
            return;
        }
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
    let reply = move |reply: Reply| {
        if busy.get_untracked() || !video_ready.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let mut judged = false;
            match call::<Feedback>(CMD_JOURNEY_ANSWER, ReplyArgs { reply }).await {
                Ok(f) => {
                    phase.set(Phase::Judged(Box::new(f)));
                    refresh_game(game);
                    judged = true;
                }
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
            if judged && prefs.get_untracked().quick_next {
                next();
            }
        });
    };
    let reveal = move || {
        if busy.get_untracked()
            || !video_ready.get_untracked()
            || phase.get_untracked() != Phase::Asking
        {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            // The grade is always the user's: quick mode only moves on after it.
            match call::<Revealed>(CMD_JOURNEY_REVEAL, NoArgs {}).await {
                Ok(r) => phase.set(Phase::Revealed(r)),
                Err(e) => error.set(Some(e)),
            }
            busy.set(false);
        });
    };
    // A workbook action on the current question; „reveal“ actions show the answer.
    let act = move |key: String| {
        if busy.get_untracked() || !video_ready.get_untracked() {
            return;
        }
        let revealed = matches!(phase.get_untracked(), Phase::Revealed(_));
        let after = matches!(phase.get_untracked(), Phase::Judged(_));
        busy.set(true);
        spawn_local(async move {
            let args = ActionArgs {
                key,
                revealed,
                after,
            };
            match call::<ActionDone>(CMD_JOURNEY_ACTION, args).await {
                // After grading only the shown answer changes; grades stay.
                Ok(done) if after => {
                    if let Phase::Judged(mut f) = phase.get_untracked() {
                        if let Some(r) = done.revealed {
                            f.revealed = r;
                        }
                        // The next question as the action left it.
                        if let (Some(q), Step::Question(_)) = (done.next, &f.next) {
                            f.next = Step::Question(q);
                        }
                        phase.set(Phase::Judged(f));
                    }
                    busy.set(false);
                }
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

    // Keyboard: Space/Enter reveal or continue, 1–4 rate a recall.
    let keys = window_event_listener(ev::keydown, move |e| {
        if e.target()
            .and_then(|target| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(target).ok())
            .is_some_and(|element| element.closest(".tour-panel").ok().flatten().is_some())
        {
            return;
        }
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
            <button class="back" title="Beenden" on:click=move |_| screen.set(crate::start_screen())>
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
        <GameStatus status=game art=art />
        {move || error.get().map(|e| view! { <p class="notes">{e}</p> })}
        {move || {
            stage_step.get();
            if summary.get().is_some() || fields.get().is_some() {
                return None;
            }
            question
                .get_untracked()
                .map(|q| view! { <Stage question=q phase=phase /> }.into_any())
        }}
        {move || {
            if let Some(s) = summary.get() {
                return view! { <Done summary=s screen=screen art=art /> }.into_any();
            }
            if let Some(list) = fields.get() {
                return view! { <Inputs fields=list begin=begin /> }.into_any();
            }
            let Some(q) = question.get() else {
                return view! { <crate::Loading /> }.into_any();
            };
            view! {
                <section class="card panel" class:revealed=move || !matches!(phase.get(), Phase::Asking)>
                    <div class="progress">{q.room.clone()}</div>
                    <div id="tour-journey-prompt" class="prompt" tabindex="-1">{q.prompt.clone()}</div>
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
                    // The answers wait for the locus while the video plays; no status text for that.
                    <fieldset disabled=move || !video_ready.get() style="border:0;padding:0;margin:0;min-width:0">
                    {(!q.actions.is_empty())
                        .then(|| {
                            let actions = q.actions.clone();
                            view! {
                                <Show when=move || summary.get().is_none()>
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
                            view! { <Judged feedback=*f question=q.clone() next=next busy=busy art=art /> }.into_any()
                        }
                    }}
                    </fieldset>
                </section>
            }
                .into_any()
        }}
    }
}

fn trophy_name(trophy: Trophy) -> &'static str {
    match trophy {
        Trophy::FirstJourney => "Erste Journey",
        Trophy::PerfectJourney => "Fehlerfreie Journey",
        Trophy::HundredXp => "100 XP",
        Trophy::SevenDayStreak => "7 Tage gelernt",
    }
}

#[component]
fn GameStatus(
    status: RwSignal<Option<GamificationStatus>>,
    art: RwSignal<Option<UiAssets>>,
) -> impl IntoView {
    // Answers only change the numbers: the icons are built once per artwork, not once per answer,
    // so the animated frames (and the video next to them) are not rebuilt.
    let trophies = Memo::new(move |_| {
        status.with(|game| game.as_ref().map(|game| game.trophies.clone()).unwrap_or_default())
    });
    view! {
        <Show when=move || status.with(Option::is_some)>
            <aside class="game-status card" aria-label="Lernfortschritt">
                <div class="game-numbers">
                    <span class="game-stat" title="Erfahrungspunkte">
                        {move || art.get().map(|a| view! { <span class="game-icon"><Sply media=a.trophy still=true /></span> })}
                        <strong>{move || status.with(|game| game.as_ref().map(|game| format!("{} XP", game.xp)).unwrap_or_default())}</strong>
                    </span>
                    <span class="game-stat" title="Richtige Antworten in Folge">
                        {move || art.get().map(|a| view! { <span class="game-icon"><Sply media=a.combo still=true /></span> })}
                        <strong>{move || status.with(|game| game.as_ref().map(|game| format!("{} Combo", game.combo)).unwrap_or_default())}</strong>
                    </span>
                    <span class="game-hearts" aria-label=move || status.with(|game| format!("{} von 5 Herzen", game.as_ref().map_or(0, |game| game.hearts.min(5))))>
                        {(0..5u8)
                            .map(|index| {
                                view! {
                                    <span class:spent=move || status.with(|game| game.as_ref().is_some_and(|game| index >= game.hearts)) class="game-icon" aria-hidden="true">
                                        {move || art.get().map(|a| view! { <Sply media=a.heart still=true /> })}
                                    </span>
                                }
                            })
                            .collect_view()}
                    </span>
                </div>
                <div class="game-daily">
                    <div class="game-daily-label">
                        {move || art.get().map(|a| view! { <span class="game-icon" aria-hidden="true"><Sply media=a.streak still=true /></span> })}
                        <label for="game-daily-progress">{move || status.with(|game| game.as_ref().map(|game| format!("Tagesziel: {} von {} richtig · {} Tage in Folge", game.daily_correct, game.daily_goal, game.streak)).unwrap_or_default())}</label>
                    </div>
                    <progress
                        id="game-daily-progress"
                        max=move || status.with(|game| game.as_ref().map_or(1, |game| game.daily_goal.max(1)))
                        value=move || status.with(|game| game.as_ref().map_or(0, |game| game.daily_correct.min(game.daily_goal.max(1))))
                    ></progress>
                </div>
                {move || {
                    (!trophies.get().is_empty()).then(|| view! {
                        <div class="game-trophies" aria-label="Erfolge">
                            {trophies.get().into_iter().map(|trophy| view! {
                                <span class="game-trophy">{move || art.get().map(|a| view! { <span class="game-icon"><Sply media=a.trophy still=true /></span> })}{trophy_name(trophy)}</span>
                            }).collect_view()}
                        </div>
                    })
                }}
            </aside>
        </Show>
    }
}

/// The room with the asked place marked (state „fällig“).
#[component]
fn Stage(question: Question, phase: RwSignal<Phase>) -> impl IntoView {
    expect_context::<VideoReady>().0.set(true);
    let video_position = expect_context::<VideoPosition>().0;
    let display = crate::locus_display::signal();
    let leads = crate::locus_display::leads();
    let timed = question.time.is_some();
    let nearby = question.nearby.clone();
    let marker = question.marker.clone();
    let zoom = question.zoom.clamp(1.0, 4.0);
    let number = question.n;
    let (locus_x, locus_y) = (question.x, question.y);
    // Reverse questions ask for the place: the stage takes the click instead of a reveal.
    let reverse = question.picture.is_some() && question.kind == AnswerKind::Recall;
    let spot = expect_context::<GuessSpot>().0;
    // A video holds at the locus; a click before that would not be about this place.
    let video_ready = expect_context::<VideoReady>().0;
    view! {
        <div class="stage" on:click=move |event: web_sys::MouseEvent| {
            if !reverse || !video_ready.get_untracked() || !matches!(phase.get_untracked(), Phase::Asking) {
                return;
            }
            let Some(stage) = event
                .current_target()
                .and_then(|target| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(target).ok())
            else {
                return;
            };
            let rect = stage.get_bounding_client_rect();
            if rect.width() <= 0.0 || rect.height() <= 0.0 {
                return;
            }
            let click_x = (f64::from(event.client_x()) - rect.left()) / rect.width();
            let click_y = (f64::from(event.client_y()) - rect.top()) / rect.height();
            // The radius is a share of the stage width, so it means the same on phone and desktop.
            let dx = (click_x - stage_fraction(locus_x, locus_x, zoom)) * rect.width();
            let dy = (click_y - stage_fraction(locus_y, locus_y, zoom)) * rect.height();
            let radius = crate::locus_display::signal().get_untracked().reverse_radius * rect.width();
            spot.set(Some(Guess {
                n: number,
                x: room_fraction(click_x, locus_x, zoom),
                y: room_fraction(click_y, locus_y, zoom),
                hit: dx.hypot(dy) <= radius,
            }));
        }>
            {(!question.video_preview.is_empty()).then(|| view! {
                <div class="video-start-preview" role="note">
                    <strong>"Startvorschau · Orte bei 0 s"</strong>
                    <ul>{question.video_preview.clone().into_iter().map(|locus| view! {
                        <li><span>{locus.place}</span>{(!locus.item.is_empty()).then(|| view! { " · " <span>{locus.item}</span> })}</li>
                    }).collect_view()}</ul>
                </div>
            })}
            // The whole room zooms in on the locus; the marker keeps its size.
            <div class="stage-view" style:transform=zoom_view(question.x, question.y, zoom)>
                {question.backdrop.clone().map(|m| {
                    if m.kind == locora_app_api::MediaKind::Video {
                        view! { <VideoAtLocus media=m time=question.time phase=phase /> }.into_any()
                    } else if m.kind == locora_app_api::MediaKind::Sply && question.time.is_some() {
                        view! { <SplyAtLocus media=m time=question.time /> }.into_any()
                    } else {
                        view! { <Shown media=m class="backdrop" /> }.into_any()
                    }
                })}
                {move || {
                    // A locus with a picture shows the picture; without one, its text stands in.
                    phase.get().stage_answer().cloned().map(|r| match r.picture {
                        Some(m) => view! {
                            <crate::LocusPicture media=m x=question.x y=question.y size=r.image_size />
                        }.into_any(),
                        None => view! {
                            <div class="locus-text" style:left=format!("{}%", question.x * 100.0) style:top=format!("{}%", question.y * 100.0)>
                                {r.solution.clone()}
                            </div>
                        }.into_any(),
                    })
                }}
                {nearby.into_iter().map(|locus| {
                    let id = locus.id.clone();
                    let offset = locus.offset;
                    let time = locus.time;
                    let (x, y, size) = (locus.x, locus.y, locus.image_size);
                    let picture = locus.picture.clone();
                    view! {
                        <div class="marker"
                            style:display=move || {
                                let prefs = display.get();
                                let lead = leads.get().get(&id).copied().unwrap_or(prefs.video_lead);
                                // Earlier loci show their revealed picture instead of a marker.
                                let visible = crate::locus_display::visible(offset, prefs.previous, prefs.next)
                                    && !(prefs.keep_previous && offset < 0)
                                    && !(offset == 0 && phase.get().stage_answer().is_some() && prefs.hide_revealed)
                                    && (!timed || crate::locus_display::at_video_time(time, video_position.get(), lead));
                                if visible { "block" } else { "none" }
                            }
                            style:left=format!("{}%", locus.x * 100.0)
                            style:top=format!("{}%", locus.y * 100.0)
                            style:transform=format!("translate(-50%, -50%) scale({})", 1.0 / zoom)
                        >
                            <Sply media=marker.clone() state=Signal::stored("faellig") />
                            <span class="num" aria-hidden="true" style:display=move || if display.get().numbers { "grid" } else { "none" }>{locus.number}</span>
                        </div>
                        {move || {
                            let prefs = display.get();
                            if !(prefs.keep_previous && offset < 0) { return None; }
                            picture.clone().map(|media| view! {
                                <crate::LocusPicture media=media x=x y=y size=size />
                            })
                        }}
                    }
                }).collect_view()}
                {move || {
                    // The guess stays on the room, so it zooms along with the locus.
                    spot.get()
                        .filter(|guess| guess.n == number)
                        .map(|guess| view! {
                            <div class="guess-marker" class:hit=guess.hit class:miss=!guess.hit
                                style:left=format!("{}%", guess.x * 100.0)
                                style:top=format!("{}%", guess.y * 100.0)></div>
                        })
                }}
            </div>
        </div>
    }
}

/// Splinely's own timeline plays to the locus and holds that exact position.
#[component]
fn SplyAtLocus(media: locora_app_api::Media, time: Option<f64>) -> impl IntoView {
    let ready = expect_context::<VideoReady>().0;
    ready.set(false);
    let progress = expect_context::<VideoProgress>().0;
    let position = expect_context::<VideoPosition>().0;
    let error = RwSignal::new(false);
    let target = time.unwrap_or(0.0);
    let key = media.hash.clone();
    let start = video_segment_start(progress.get_value().as_ref(), &key, target);
    let command = RwSignal::new(TimedCommand {
        serial: 0,
        time: target,
        start: Some(start),
        play: true,
    });
    view! {
        <Sply media=media class="backdrop" timed=command
            on_timed_position=Box::new(move |time| position.set(time))
            on_timed_at=Box::new(move |time| {
                position.set(time);
                progress.set_value(Some((key.clone(), time)));
                ready.set(true);
            })
            on_timed_error=Box::new(move || {
                error.set(true);
                ready.set(true);
            })
        />
        <Show when=move || error.get()>
            <p class="notes" role="alert">"Splinely-Animation oder gespeicherte Locus-Zeit konnte nicht geladen werden."</p>
        </Show>
    }
}

/// Play to the question's timestamp, then hold until the answer is graded.
#[component]
fn VideoAtLocus(
    media: locora_app_api::Media,
    time: Option<f64>,
    phase: RwSignal<Phase>,
) -> impl IntoView {
    let player = NodeRef::<leptos::html::Video>::new();
    let error = RwSignal::new(false);
    let fallback = RwSignal::new(false);
    let ready = expect_context::<VideoReady>().0;
    ready.set(false);
    let progress = expect_context::<VideoProgress>().0;
    let position = expect_context::<VideoPosition>().0;
    let key = StoredValue::new(media.hash.clone());
    let frame_watch = crate::video_frames::new_id();
    let stop_at_locus = move || {
        if let Some(player) = player.get_untracked() {
            position.set(player.current_time());
        }
        if let (Some(player), Some(target)) = (player.get_untracked(), time)
            && player.ready_state() >= 1
            && target.is_finite()
            && target >= 0.0
            && !matches!(phase.get_untracked(), Phase::Judged(_))
            && player.current_time() >= target
        {
            let _ = player.pause();
            if player.current_time() != target {
                player.set_current_time(target);
            }
            progress.set_value(Some((key.get_value(), target)));
            ready.set(true);
        }
    };
    // timeupdate alone can lag by hundreds of milliseconds. Poll near frame
    // cadence only while approaching this locus, then stop the interval.
    let timer = set_interval_with_handle(stop_at_locus, std::time::Duration::from_millis(16)).ok();
    Effect::new(move |_| {
        if ready.get() {
            if let Some(timer) = timer {
                timer.clear();
            }
            crate::video_frames::stop(frame_watch);
        }
    });
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
        crate::video_frames::stop(frame_watch);
    });
    view! {
        <VideoBackdrop
            media=media.clone()
            class="backdrop"
            // No native bar with its seek slider: the journey moves the video itself.
            controls=false
            muted=true
            preload="auto".to_string()
            autoplay=true
            node_ref=player
            on_loaded=Box::new(move || {
                if let Some(video) = player.get_untracked() {
                    fit_stage(video.as_ref(), f64::from(video.video_width()), f64::from(video.video_height()));
                }
                on_metadata(player, time, phase, progress, key, ready, (error, fallback));
                if !ready.get_untracked()
                    && let Some(video) = player.get_untracked()
                {
                    crate::video_frames::start(frame_watch, &video, stop_at_locus);
                }
            })
            on_timeupdate=Box::new(stop_at_locus)
            on_error=Box::new(move || { error.set(true); ready.set(true); })
        />
        <Show when=move || error.get()>
            <p class="notes" role="alert">"Video oder gespeicherte Locus-Zeit konnte nicht geladen werden."</p>
        </Show>
        <Show when=move || fallback.get()>
            <p class="notes" role="status">"Automatische Wiedergabe nicht möglich; direkt zum Locus gesprungen."</p>
        </Show>
    }
}

fn video_segment_start(previous: Option<&(String, f64)>, key: &str, target: f64) -> f64 {
    previous
        .filter(|(old_key, t)| old_key == key && t.is_finite() && *t >= 0.0 && *t <= target)
        .map_or(0.0, |(_, t)| *t)
}

/// Seek to the question's segment once the video's metadata is loaded.
fn on_metadata(
    player: NodeRef<leptos::html::Video>,
    time: Option<f64>,
    _phase: RwSignal<Phase>,
    progress: StoredValue<Option<(String, f64)>>,
    key: StoredValue<String>,
    ready: RwSignal<bool>,
    flags: (RwSignal<bool>, RwSignal<bool>),
) {
    let (error, fallback) = flags;
    let Some(player) = player.get_untracked() else {
        return;
    };
    let duration = player.duration();
    match time.filter(|t| t.is_finite() && *t >= 0.0 && (!duration.is_finite() || *t <= duration)) {
        Some(target) => {
            let start =
                video_segment_start(progress.get_value().as_ref(), &key.get_value(), target);
            player.set_current_time(start);
            if start == target {
                let _ = player.pause();
                progress.set_value(Some((key.get_value(), target)));
                ready.set(true);
                return;
            }
            let playing = player.play();
            spawn_local(async move {
                let failed = match playing {
                    Ok(promise) => wasm_bindgen_futures::JsFuture::from(promise).await.is_err(),
                    Err(_) => true,
                };
                if failed && player.is_connected() && !ready.get_untracked() {
                    let _ = player.pause();
                    player.set_current_time(target);
                    progress.set_value(Some((key.get_value(), target)));
                    fallback.set(true);
                    ready.set(true);
                }
            });
        }
        None => {
            error.set(true);
            ready.set(true);
        }
    }
}

/// CSS transform that scales the room by `zoom` around its top-left corner and
/// moves the locus (x, y in 0..1) to the centre – as far as the room allows,
/// so the view never leaves the backdrop.
fn zoom_view(x: f64, y: f64, zoom: f64) -> String {
    let shift = |p: f64| (0.5 - p * zoom).clamp(1.0 - zoom, 0.0) * 100.0;
    format!("translate({}%, {}%) scale({zoom})", shift(x), shift(y))
}

#[component]
fn Ask(
    question: Question,
    busy: RwSignal<bool>,
    reveal: impl Fn() + Copy + Send + Sync + 'static,
    reply: impl Fn(Reply) + Copy + Send + Sync + 'static,
) -> impl IntoView {
    match question.kind {
        AnswerKind::Recall => {
            // A reverse question asks for the place: the stage takes the click, and this line says what it was.
            let reverse = question.picture.is_some();
            let number = question.n;
            let spot = expect_context::<GuessSpot>().0;
            view! {
                {reverse.then(|| view! {
                    <p class="progress">
                        {move || match spot.get().filter(|guess| guess.n == number) {
                            Some(guess) if guess.hit => "Treffer – der Ort liegt im Radius.",
                            Some(_) => "Daneben – der Ort ist markiert. Du kannst trotzdem aufdecken.",
                            None => "Klick auf die Bühne, wo der Inhalt war.",
                        }}
                    </p>
                })}
                <button class="primary" disabled=move || busy.get() on:click=move |_| reveal()>
                    "Aufdecken (Leertaste)"
                </button>
            }
            .into_any()
        }
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
            let toggle = move |i: u32| {
                picked.update(|p| {
                    if let Some(at) = p.iter().position(|x| *x == i) {
                        p.remove(at);
                    } else {
                        p.push(i);
                    }
                })
            };
            // PLAN 6.3: keys 1–9 toggle options, Enter checks. Only mounted while asking.
            let count = question.options.len().min(9) as u32;
            let grid = question.options.len() > 3;
            let keys = window_event_listener(leptos::ev::keydown, move |e| {
                if busy.get_untracked() || e.alt_key() || e.ctrl_key() || e.meta_key() || e.repeat()
                {
                    return;
                }
                // A focused button already handles Enter itself (no double submit).
                let control = e
                    .target()
                    .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(t).ok())
                    .is_some_and(|el| {
                        matches!(el.tag_name().as_str(), "BUTTON" | "TEXTAREA")
                            || el.get_attribute("type").as_deref() == Some("text")
                    });
                match e.key().as_str() {
                    "Enter" if !control && !picked.get_untracked().is_empty() => {
                        e.prevent_default();
                        reply(Reply::Quiz(picked.get_untracked()));
                    }
                    key => {
                        if let Some(n) = key.parse::<u32>().ok().filter(|n| (1..=count).contains(n))
                        {
                            e.prevent_default();
                            toggle(n - 1);
                        }
                    }
                }
            });
            on_cleanup(move || keys.remove());
            view! {
                <p class="progress">"Wähle alle richtigen Antworten: Zahlen 1–9 wählen, Enter prüft. Es können auch mehrere richtig sein."</p>
                <div class="options" class:grid=grid>
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
                                        on:change=move |_| toggle(i)
                                    />
                                    {(i < 9).then(|| view! { <kbd>{i + 1}</kbd> })}
                                    {text.clone()}
                                </label>
                            }
                        })
                        .collect_view()}
                </div>
                <div class="progress">"Alle richtigen wählen (Tasten 1–9, Enter prüft)."</div>
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
fn Judged(
    feedback: Feedback,
    question: Question,
    next: impl Fn() + 'static,
    busy: RwSignal<bool>,
    art: RwSignal<Option<UiAssets>>,
) -> impl IntoView {
    let game = feedback.game;
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
                let correct = feedback.correct;
                let picture = move || {
                    art.get()
                        .map(|a| if correct { a.right } else { a.again })
                        .map(|m| view! { <div class="feedback-art"><Sply media=m /></div> })
                };
                if correct {
                    view! { <div class="verdict ok">{picture}"Richtig"</div> }.into_any()
                } else {
                    view! { <div class="verdict again">{picture}"Nochmal"</div> }.into_any()
                }
            })}
        <Answer solution=r.solution note=r.note picture=r.picture />
        {game.map(|game| view! { <p class="progress game-status">{game_status(game)}</p> })}
        {(!right.is_empty())
            .then(|| {
                view! { <div class="progress">{format!("Richtig: {}", right.join(", "))}</div> }
            })}
        <button class="primary" disabled=move || busy.get() on:click=move |_| next()>
            "Weiter (Leertaste)"
        </button>
    }
}

/// Combo and hearts as one line, e.g. "Combo 3 · Herzen ♥♥♥♡♡".
fn game_status(game: GameView) -> String {
    const HEARTS: usize = 5;
    let held = usize::from(game.hearts).min(HEARTS);
    format!(
        "Combo {} · Herzen {}{}",
        game.combo,
        "♥".repeat(held),
        "♡".repeat(HEARTS - held)
    )
}

#[component]
fn Done(
    summary: Summary,
    screen: RwSignal<Screen>,
    art: RwSignal<Option<UiAssets>>,
) -> impl IntoView {
    let asked = summary.asked;
    view! {
        <section class="card panel">
            {move || {
                art.get()
                    .filter(|_| asked > 0)
                    .map(|a| view! { <div class="done-art"><Sply media=a.done /></div> })
            }}
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
            <button class="primary" on:click=move |_| screen.set(crate::start_screen())>
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
    fn game_status_shows_combo_and_hearts() {
        assert_eq!(
            game_status(GameView {
                combo: 3,
                hearts: 3
            }),
            "Combo 3 · Herzen ♥♥♥♡♡"
        );
        assert_eq!(
            game_status(GameView {
                combo: 0,
                hearts: 9
            }),
            "Combo 0 · Herzen ♥♥♥♥♥"
        );
    }

    #[test]
    fn video_segments_continue_only_for_same_media_in_forward_order() {
        assert_eq!(video_segment_start(None, "a", 5.0), 0.0);
        assert_eq!(video_segment_start(Some(&("a".into(), 2.5)), "a", 5.0), 2.5);
        assert_eq!(video_segment_start(Some(&("a".into(), 5.0)), "a", 5.0), 5.0);
        assert_eq!(video_segment_start(Some(&("a".into(), 8.0)), "a", 5.0), 0.0);
        assert_eq!(video_segment_start(Some(&("b".into(), 2.5)), "a", 5.0), 0.0);
        for invalid in [f64::NAN, f64::INFINITY, -1.0] {
            assert_eq!(
                video_segment_start(Some(&("a".into(), invalid)), "a", 5.0),
                0.0
            );
        }
    }

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
            game: None,
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
                    value: if f.kind == "number" {
                        decimal_point(&v.get_untracked())
                    } else {
                        v.get_untracked()
                    },
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
                                    // Text, not type=number: that would empty
                                    // workbook-valid literals such as `+1`.
                                    <input
                                        type="text"
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

/// German decimal comma → point (`1,5` → `1.5`), the form the workbook parses.
/// Anything else is left alone for the backend to validate.
fn decimal_point(text: &str) -> String {
    let text = text.trim();
    if text.matches(',').count() == 1 && !text.contains('.') {
        text.replace(',', ".")
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod input_tests {
    use super::decimal_point;

    #[test]
    fn decimal_comma_becomes_point() {
        assert_eq!(decimal_point(" 1,5 "), "1.5");
        assert_eq!(decimal_point("+1"), "+1");
        assert_eq!(decimal_point("1.5"), "1.5");
        assert_eq!(decimal_point("1,000.5"), "1,000.5");
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::zoom_view;

    #[test]
    fn zoom_centres_the_locus_inside_the_room() {
        assert_eq!(zoom_view(0.3, 0.7, 1.0), "translate(0%, 0%) scale(1)");
        // Centre stays centre.
        assert_eq!(zoom_view(0.5, 0.5, 2.0), "translate(-50%, -50%) scale(2)");
        // Near the edge the view stops at the backdrop's border.
        assert_eq!(zoom_view(0.0, 1.0, 2.0), "translate(0%, -100%) scale(2)");
    }
}
