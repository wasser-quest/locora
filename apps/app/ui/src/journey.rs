//! A journey: one question at a time. Recall: Space reveals, 1–4 grade.
//! Guess: type and Enter (empty Enter reveals). Quiz: tick and confirm.
//! Revealing shows the locus picture, because it belongs to the answer.

use crate::i18n::t;
use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    ActionDone, AnswerKind, CMD_GAME_SEEN, CMD_GAMIFICATION_STATUS, CMD_JOURNEY_ACTION,
    CMD_JOURNEY_ANSWER, CMD_JOURNEY_INPUTS, CMD_JOURNEY_REVEAL, CMD_JOURNEY_START, Feedback,
    GameSeenArgs, GameStatusArgs, GameView, GamificationStatus, Goal, InputField, InputValue,
    JourneyStart, Question, Reply, Revealed, Step, Summary, Today, UiAssets, Week,
};
use serde::Serialize;

use crate::share::CardKind;
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

/// The picture the video showed at the last locus. The next question's video shows it until its own
/// frame is there, so the stage never flashes empty between places.
#[derive(Clone, Copy)]
struct LastFrame(StoredValue<Option<String>>);

/// Reads the game's numbers again, and celebrates the trophies this device's answers and journey
/// just earned (`session.unlocked`; never what a sync brought). Palace medals are celebrated
/// with the end card, which knows their palace's name.
fn refresh_game(status: RwSignal<Option<GamificationStatus>>, celebrate: impl Fn(&str) + 'static) {
    spawn_local(async move {
        let args = GameStatusArgs { full: false };
        if let Ok(value) = call::<Option<GamificationStatus>>(CMD_GAMIFICATION_STATUS, args).await {
            if let Some(session) = value.as_ref().and_then(|game| game.session.as_ref()) {
                for id in session.unlocked.iter().filter(|id| id.starts_with("game-")) {
                    celebrate(id);
                }
            }
            status.set(value);
        }
    });
}

/// Today's due goal in words, and the loci back for practice.
pub fn today_text(today: &Today) -> String {
    let goal = match today.goal {
        Goal::Open => crate::i18n::tf(
            "Heute fällig: {0} von {1} erledigt",
            &[&today.due_done, &(today.due_done + today.due_open)],
        ),
        Goal::AllDone => t("Alle fälligen Wiederholungen erledigt").to_owned(),
        Goal::Free => t("Heute ist nichts fällig").to_owned(),
        Goal::Enough => crate::i18n::tf(
            "{0} fällige Wiederholungen erledigt – der Rest kann warten",
            &[&today.due_done],
        ),
    };
    match today.repeat_open {
        0 => goal,
        1 => format!("{goal} · {}", t("1 Ort zum Nachüben (ohne XP)")),
        repeat => format!(
            "{goal} · {}",
            crate::i18n::tf("{0} Orte zum Nachüben (ohne XP)", &[&repeat])
        ),
    }
}

/// The weekly run, when there is something to say: never what was lost.
pub fn week_run_text(week: &Week) -> Option<String> {
    match (week.streak, week.missed_last) {
        (0, true) if week.best == 1 => {
            Some(t("Neue Woche, neues Ziel. Dein Bestwert: 1 Woche in Folge.").to_owned())
        }
        (0, true) => Some(crate::i18n::tf(
            "Neue Woche, neues Ziel. Dein Bestwert: {0} Wochen in Folge.",
            &[&week.best],
        )),
        (0, false) => None,
        (1, _) => Some(t("Wochenziel erreicht").to_owned()),
        (streak, _) => Some(crate::i18n::tf(
            "Wochenziel {0} Wochen in Folge erreicht",
            &[&streak],
        )),
    }
}

/// This week's learning days against its target (beyond it the week is simply reached), and the
/// days with nothing due that lowered the target.
pub fn week_text(week: &Week) -> String {
    let days = week.days.min(week.target);
    let main = if days == 1 {
        crate::i18n::tf("Diese Woche: 1 von {0} Lerntagen", &[&week.target])
    } else {
        crate::i18n::tf("Diese Woche: {0} von {1} Lerntagen", &[&days, &week.target])
    };
    match week.free {
        0 => main,
        1 => format!("{main} · {}", t("1 Tag ohne Fälliges")),
        free => format!(
            "{main} · {}",
            crate::i18n::tf("{0} Tage ohne Fälliges", &[&free])
        ),
    }
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
    provide_context(LastFrame(StoredValue::new(None)));
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
    // New trophies and medals are celebrated once each, in the order earned (looked up here: a
    // task after an await has no owner).
    let celebration = use_context::<crate::achievements::Celebration>();
    let celebrated = StoredValue::new(std::collections::BTreeSet::<String>::new());
    let celebrate = move |id: &str| {
        let new = celebrated
            .try_update_value(|seen| seen.insert(id.to_owned()))
            .unwrap_or(false);
        if let (true, Some(celebration)) = (new, celebration) {
            celebration.celebrate(id);
        }
    };
    // Feedback animations (assets/ui); the journey works without them.
    let art = crate::art();
    // The palaces this walk asked in: a card may name the palace when it was one only.
    let palaces = StoredValue::new(std::collections::BTreeSet::<String>::new());
    Effect::new(move |_| {
        if let Some(palace) = question.with(|q| q.as_ref().map(|q| q.palace.clone())) {
            palaces.update_value(|all| {
                all.insert(palace);
            });
        }
    });

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
                    refresh_game(game, celebrate);
                }
                Ok(Step::Done(s)) => {
                    summary.set(Some(s));
                    refresh_game(game, celebrate);
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
    // The end card; what this journey earned and was not celebrated yet is (trophies of single
    // answers were when the bar read the numbers again).
    let finished = move |s: Summary| {
        for id in &s.new_trophies {
            celebrate(id);
        }
        summary.set(Some(s));
    };
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
            Step::Done(s) => finished(s),
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
                    refresh_game(game, celebrate);
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
    // The app tour's grading step needs the answer shown: on Simo's example it reveals it.
    let tour = use_context::<crate::TourStep>();
    let tour_palace = start.get_value().palace_id.clone().unwrap_or_default();
    // The game's explanation waits until the tour is over, and never shows in Simo's example.
    let demo = crate::is_tour_demo(&tour_palace);
    let explain =
        Signal::derive(move || !demo && tour.is_none_or(|tour| tour.0.with(Option::is_none)));
    Effect::new(move |_| {
        if crate::tour_target_in(tour, &tour_palace).as_deref() == Some("tour-journey-grade")
            && phase.get() == Phase::Asking
            && question.with(|q| q.as_ref().is_some_and(|q| q.kind == AnswerKind::Recall))
            && video_ready.get()
        {
            reveal();
        }
    });
    // A time limit per question: a question still unanswered when the time is up counts as
    // a wrong answer (forfeit). A timer that fires after the question changed does nothing.
    if let Some(seconds) = start.get_value().time_limit.filter(|seconds| *seconds > 0) {
        Effect::new(move |_| {
            let Some(step) = stage_step.get() else {
                return;
            };
            set_timeout(
                move || {
                    if stage_step.get_untracked() == Some(step)
                        && phase.get_untracked() == Phase::Asking
                    {
                        reply(Reply::Forfeit);
                    }
                },
                std::time::Duration::from_secs(u64::from(seconds)),
            );
        });
    }
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
            .is_some_and(|element| {
                element
                    .closest(".tour-panel, .game-intro")
                    .ok()
                    .flatten()
                    .is_some()
            })
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
            // Space grades like 1 ("again"): nobody claims to have known it by holding Space.
            (Phase::Revealed(_), " ") => {
                e.prevent_default();
                reply(Reply::Recall(1));
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
            <button class="back" title=t("Beenden") on:click=move |_| screen.set(crate::start_screen())>
                "←"
            </button>
            <h1>
                {move || question.get().map(|q| q.palace).unwrap_or_else(|| t("Journey").into())}
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
        <GameStatus status=game art=art explain=explain />
        {move || {
            // Restarts with every question: the bar is rebuilt when the step changes.
            let step = stage_step.get()?;
            let seconds = start.get_value().time_limit.filter(|seconds| *seconds > 0)?;
            (phase.get() == Phase::Asking).then(|| {
                view! {
                    <div class="timer" aria-hidden="true" data-step=step>
                        <span style=format!("animation-duration: {seconds}s")></span>
                    </div>
                }
            })
        }}
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
                let palace = palaces.with_value(|all| (all.len() == 1).then(|| all.iter().next().cloned()).flatten());
                let week = game.with_untracked(|game| game.as_ref().map(|game| game.week.clone()));
                return view! { <Done summary=s screen=screen art=art palace=palace week=week /> }.into_any();
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
                                <GuessVerdict number=q.n />
                                <Answer solution=r.solution note=r.note picture=r.picture audio=r.audio autoplay=true />
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

/// Whether the screen is narrow (a phone): the explanation starts folded there.
fn narrow() -> bool {
    web_sys::window()
        .and_then(|window| window.inner_width().ok())
        .and_then(|width| width.as_f64())
        .is_some_and(|width| width <= 560.0)
}

/// The journey bar: XP, the week, today's due goal and the weekly run. `explain` says whether
/// the one-time explanation may show (not during the app tour or in its example palace).
#[component]
fn GameStatus(
    status: RwSignal<Option<GamificationStatus>>,
    art: RwSignal<Option<UiAssets>>,
    #[prop(into)] explain: Signal<bool>,
) -> impl IntoView {
    let display = crate::locus_display::signal();
    let folded = narrow();
    // Answers only change the numbers: the icons are built once per artwork, not once per answer,
    // so the animated frames (and the video next to them) are not rebuilt.
    let read = move |f: fn(&GamificationStatus) -> String| {
        move || status.with(|game| game.as_ref().map(f).unwrap_or_default())
    };
    let dismiss = move |hide: bool| {
        if hide {
            display.update(|p| p.gamification = false);
            crate::locus_display::save(display.get_untracked());
        }
        status.update(|game| {
            if let Some(game) = game {
                game.intro_seen = true;
            }
        });
        spawn_local(async move {
            let args = GameSeenArgs {
                hint: "game-intro".into(),
            };
            let _ = call::<()>(CMD_GAME_SEEN, args).await;
        });
    };
    view! {
        <Show when=move || status.with(Option::is_some) && display.get().gamification>
            <aside class="game-status card" aria-label=t("Lernfortschritt")>
                <div class="game-numbers">
                    <span class="game-stat game-xp" title=t("Erfahrungspunkte") aria-label=read(|game| crate::i18n::tf("{0} XP", &[&game.xp]))>
                        {move || art.get().map(|a| view! { <span class="game-icon" aria-hidden="true"><Sply media=a.trophy still=true /></span> })}
                        <strong>{read(|game| crate::i18n::tf("{0} XP", &[&game.xp]))}</strong>
                    </span>
                    <span class="game-stat game-week">
                        {move || art.get().map(|a| view! { <span class="game-icon" aria-hidden="true"><Sply media=a.streak still=true /></span> })}
                        {read(|game| week_text(&game.week))}
                    </span>
                </div>
                <div class="game-daily">
                    <label for="game-daily-progress">{read(|game| today_text(&game.today))}</label>
                    // Nothing due: no bar that looks full (or empty) for a day without a goal.
                    <Show when=move || status.with(|game| game.as_ref().is_some_and(|game| game.today.goal != Goal::Free))>
                        <progress
                            id="game-daily-progress"
                            max=move || status.with(|game| game.as_ref().map_or(1, |game| (game.today.due_done + game.today.due_open).max(1)))
                            value=move || status.with(|game| game.as_ref().map_or(0, |game| match game.today.goal {
                                Goal::AllDone => (game.today.due_done + game.today.due_open).max(1),
                                _ => game.today.due_done,
                            }))
                        ></progress>
                    </Show>
                </div>
                {move || status.with(|game| game.as_ref().and_then(|game| week_run_text(&game.week))).map(|text| view! {
                    <p class="game-run">{text}</p>
                })}
            </aside>
            // Folded to one line on a phone, so the question stays in view.
            {move || (explain.get() && status.with(|game| game.as_ref().is_some_and(|game| !game.intro_seen))).then(|| view! {
                <aside class="game-intro card" role="note" aria-labelledby="game-intro-title">
                    <details open=!folded>
                        <summary>
                            <h2 id="game-intro-title">{move || if status.with(|game| game.as_ref().is_some_and(|game| game.legacy)) {
                                t("So funktionieren XP jetzt")
                            } else {
                                t("So funktionieren XP")
                            }}</h2>
                        </summary>
                        <p>{t("XP: 10 für jede fällige Wiederholung, 6 für jeden neuen Ort – egal, wie du bewertest.")}</p>
                        <p>{t("Früheres Üben bringt keine XP; dein Wiederholungsplan entscheidet, was fällig ist.")}</p>
                        <p>{move || {
                            let goal = status.with(|game| game.as_ref().map_or(4, |game| game.week.goal));
                            if goal == 1 {
                                t("Wochenziel: an 1 Tag pro Woche lernen. Ein verpasster Tag kostet nichts.").to_owned()
                            } else {
                                crate::i18n::tf(
                                    "Wochenziel: an {0} Tagen pro Woche lernen. Ein verpasster Tag kostet nichts.",
                                    &[&goal],
                                )
                            }
                        }}</p>
                    </details>
                    <div class="actions">
                        <button class="primary" on:click=move |_| dismiss(false)>{t("Verstanden")}</button>
                        <button on:click=move |_| dismiss(true)>{t("Leiste ausblenden")}</button>
                    </div>
                </aside>
            })}
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
                    <strong>{t("Startvorschau · Orte bei 0 s")}</strong>
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
                    // The guess stays on the room, so it zooms along with the locus. Whether it hit shows
                    // only once the answer is revealed: before, a new click just moves the choice.
                    let decided = !matches!(phase.get(), Phase::Asking);
                    spot.get()
                        .filter(|guess| guess.n == number)
                        .map(|guess| view! {
                            <div class="guess-marker" class:hit=decided && guess.hit class:miss=decided && !guess.hit
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
            <p class="notes" role="alert">{t("Splinely-Animation oder gespeicherte Locus-Zeit konnte nicht geladen werden.")}</p>
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
    let last_frame = expect_context::<LastFrame>().0;
    let poster = last_frame.get_value();
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
    // A device without a decoder for the video does not always say so (the player keeps waiting
    // without an error): after 15 seconds without metadata the journey goes on without the video,
    // so answers and grades are never blocked.
    let stalled = RwSignal::new(false);
    let waited = StoredValue::new(0u32);
    let watchdog = set_interval_with_handle(
        move || {
            let Some(video) = player.get_untracked() else {
                return;
            };
            if ready.get_untracked() || video.ready_state() > 0 {
                return;
            }
            let seconds = waited.get_value() + 1;
            waited.set_value(seconds);
            if seconds >= 15 {
                stalled.set(true);
                ready.set(true);
            }
        },
        std::time::Duration::from_secs(1),
    )
    .ok();
    Effect::new(move |_| {
        if ready.get() {
            if let Some(timer) = timer {
                timer.clear();
            }
            if let Some(watchdog) = watchdog {
                watchdog.clear();
            }
            crate::video_frames::stop(frame_watch);
            // Held at the locus: this picture bridges to the next question's video.
            if let Some(frame) = player
                .get_untracked()
                .and_then(|video| crate::sply::current_frame(&video))
            {
                last_frame.set_value(Some(frame));
            }
        }
    });
    on_cleanup(move || {
        if let Some(timer) = timer {
            timer.clear();
        }
        if let Some(watchdog) = watchdog {
            watchdog.clear();
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
            poster=poster.unwrap_or_default()
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
            <p class="notes" role="alert">{t("Video oder gespeicherte Locus-Zeit konnte nicht geladen werden.")}</p>
        </Show>
        <Show when=move || stalled.get() && !error.get()>
            <p class="notes" role="alert">{t("Das Video lädt nicht; die Reise geht ohne Video weiter.")}</p>
        </Show>
        <Show when=move || fallback.get()>
            <p class="notes" role="status">{t("Automatische Wiedergabe nicht möglich; direkt zum Locus gesprungen.")}</p>
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
                            Some(_) => t("Ort gewählt. „Aufdecken“ zeigt, ob er stimmt; ein neuer Klick verschiebt deine Wahl."),
                            None => t("Klick auf die Bühne, wo der Inhalt war."),
                        }}
                    </p>
                })}
                <button id="tour-journey-reveal" class="primary" disabled=move || busy.get() on:click=move |_| reveal()>
                    {t("Aufdecken (Leertaste)")}
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
                        placeholder=t("Antwort eingeben – Enter prüft (leer: Leertaste deckt auf)")
                        autofocus=true
                        bind:value=text
                        on:keydown=move |e| {
                            // Space in an empty field reveals, so a whole journey runs on Space alone.
                            if e.key() == " " && text.get_untracked().is_empty() {
                                e.prevent_default();
                                submit();
                            }
                        }
                    />
                </form>
                <button class="primary" disabled=move || busy.get() on:click=move |_| submit()>
                    {t("Prüfen")}
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
                    // Space checks too; with nothing chosen it reveals the answer.
                    " " if !control => {
                        e.prevent_default();
                        let chosen = picked.get_untracked();
                        reply(if chosen.is_empty() {
                            Reply::Forfeit
                        } else {
                            Reply::Quiz(chosen)
                        });
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
                <p class="progress">{t("Wähle alle richtigen Antworten: Zahlen 1–9 wählen, Enter oder Leertaste prüft. Es können auch mehrere richtig sein.")}</p>
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
                <div class="progress">{t("Alle richtigen wählen (Tasten 1–9, Enter prüft).")}</div>
                <button
                    class="primary"
                    disabled=move || busy.get() || picked.get().is_empty()
                    on:click=move |_| reply(Reply::Quiz(picked.get_untracked()))
                >
                    {t("Prüfen")}
                </button>
                <button on:click=move |_| reply(Reply::Forfeit)>{t("Aufdecken")}</button>
            }
            .into_any()
        }
    }
}

/// After revealing a reverse question: whether the chosen place was right (nothing without a choice).
#[component]
fn GuessVerdict(number: u32) -> impl IntoView {
    let spot = expect_context::<GuessSpot>().0;
    move || {
        spot.get().filter(|guess| guess.n == number).map(|guess| {
            view! {
                <p class="progress guess-verdict" class:hit=guess.hit class:miss=!guess.hit role="status">
                    {if guess.hit {
                        t("Treffer – dein Ort liegt im Radius.")
                    } else {
                        t("Daneben – deine Wahl ist rot markiert, der richtige Ort ist aufgedeckt.")
                    }}
                </p>
            }
        })
    }
}

#[component]
fn Grades(reply: impl Fn(Reply) + Copy + 'static, busy: RwSignal<bool>) -> impl IntoView {
    let grades = [
        (1u8, t("Nochmal")),
        (2, t("Schwer")),
        (3, t("Gut")),
        (4, t("Leicht")),
    ];
    view! {
        <div id="tour-journey-grade" class="grades">
            {grades
                .into_iter()
                .map(|(g, label)| {
                    view! {
                        <button disabled=move || busy.get() on:click=move |_| reply(Reply::Recall(g))>
                            {label}
                            <small>{if g == 1 { t("1 oder Leertaste").to_owned() } else { g.to_string() }}</small>
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
    let game = feedback
        .game
        .and_then(|game| game_line(game, feedback.correct));
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
                    view! { <div class="verdict ok">{picture}{t("Richtig")}</div> }.into_any()
                } else {
                    view! { <div class="verdict again">{picture}{t("Nochmal")}</div> }.into_any()
                }
            })}
        // A recalled answer was already shown (and played) when it was revealed.
        <Answer solution=r.solution note=r.note picture=r.picture audio=r.audio autoplay=automatic />
        {game.map(|line| view! { <p class="progress game-line">{line}</p> })}
        {(!right.is_empty())
            .then(|| {
                view! { <div class="progress">{crate::i18n::tf("Richtig: {0}", &[&right.join(", ")])}</div> }
            })}
        <button class="primary" disabled=move || busy.get() on:click=move |_| next()>
            {t("Weiter (Leertaste)")}
        </button>
    }
}

/// The line under an answer: the run of right Quiz and Guess answers, or after a miss a calm
/// word that it comes back. Nothing is lost; recall grades neither show nor break a run.
fn game_line(game: GameView, correct: bool) -> Option<String> {
    if !correct {
        return Some(t("Kommt bald wieder – so bleibt es hängen.").to_owned());
    }
    // A run starts at two.
    game.combo
        .filter(|combo| *combo > 1)
        .map(|combo| crate::i18n::tf("{0} in Folge", &[&combo]))
}

/// When the next review is: today, tomorrow or the date.
fn next_day(date: &str, days: Option<u32>) -> String {
    match days {
        Some(0) => t("heute").to_owned(),
        Some(1) => t("morgen").to_owned(),
        _ => date.to_owned(),
    }
}

/// „1 fällige Wiederholung · 3 neue Orte“, each in the singular where it is one.
fn done_counts(due: u32, new: u32) -> String {
    let due = if due == 1 {
        t("1 fällige Wiederholung").to_owned()
    } else {
        crate::i18n::tf("{0} fällige Wiederholungen", &[&due])
    };
    let new = if new == 1 {
        t("1 neuer Ort").to_owned()
    } else {
        crate::i18n::tf("{0} neue Orte", &[&new])
    };
    format!("{due} · {new}")
}

/// What the journey earned and what comes next, one line each.
fn summary_lines(summary: &Summary) -> Vec<String> {
    let mut lines = Vec::new();
    if summary.xp_gained.is_none() {
        return lines;
    }
    if summary.points != u64::from(summary.correct) {
        lines.push(crate::i18n::tf("Punkte: {0}", &[&summary.points]));
    }
    lines.push(done_counts(summary.due_done, summary.new_done));
    if summary.early > 0 {
        let when = summary
            .early_next
            .as_ref()
            .map(|(date, days)| next_day(date, Some(*days)));
        lines.push(match (summary.early, when) {
            (1, Some(when)) => crate::i18n::tf(
                "1 Ort war noch nicht fällig und brachte deshalb keine XP. Seine nächste Wiederholung: {0}.",
                &[&when],
            ),
            (early, Some(when)) => crate::i18n::tf(
                "{0} Orte waren noch nicht fällig und brachten deshalb keine XP. Ihre nächste Wiederholung: {1}.",
                &[&early, &when],
            ),
            (1, None) => t("1 Ort war noch nicht fällig und brachte deshalb keine XP.").to_owned(),
            (early, None) => crate::i18n::tf(
                "{0} Orte waren noch nicht fällig und brachten deshalb keine XP.",
                &[&early],
            ),
        });
    }
    match summary.repeat {
        0 => {}
        1 => lines
            .push(t("1 Ort hattest du heute schon; noch einmal üben bringt keine XP.").to_owned()),
        repeat => lines.push(crate::i18n::tf(
            "{0} Orte hattest du heute schon; noch einmal üben bringt keine XP.",
            &[&repeat],
        )),
    }
    // Today's goal, in the words of the bar.
    match (summary.goal, summary.due_left_today) {
        (Some(Goal::Free), _) => lines.push(t("Heute ist nichts fällig").to_owned()),
        (_, Some(0)) => lines.push(t("Alle fälligen Wiederholungen für heute erledigt").to_owned()),
        (_, Some(1)) => lines.push(t("Heute noch fällig: 1 Ort").to_owned()),
        (_, Some(left)) => lines.push(crate::i18n::tf("Heute noch fällig: {0} Orte", &[&left])),
        (_, None) => {}
    }
    match summary.repeat_later {
        0 => {}
        1 => lines.push(t("Kommt heute noch einmal zum Üben (ohne XP): 1 Ort").to_owned()),
        later => lines.push(crate::i18n::tf(
            "Kommt heute noch einmal zum Üben (ohne XP): {0} Orte",
            &[&later],
        )),
    }
    for id in &summary.new_trophies {
        if let Some((title, _)) = crate::achievements::describe(id) {
            lines.push(if id.starts_with("palace-") {
                crate::i18n::tf("Neue Medaille: {0}", &[&title])
            } else {
                crate::i18n::tf("Neue Trophäe: {0}", &[&title])
            });
        }
    }
    if let Some((date, count)) = &summary.next_due {
        let day = next_day(date, summary.next_due_in);
        lines.push(if *count == 1 {
            crate::i18n::tf("Nächste Wiederholung: {0} · 1 Ort", &[&day])
        } else {
            crate::i18n::tf("Nächste Wiederholung: {0} · {1} Orte", &[&day, count])
        });
    }
    lines
}

/// What the end of a journey offers to share: the journey, what it earned, and the weekly goal
/// once it is reached.
fn done_cards(summary: &Summary, palace: Option<String>, week: Option<&Week>) -> Vec<CardKind> {
    let mut cards = vec![CardKind::Journey {
        correct: summary.correct,
        asked: summary.asked,
        xp: summary.xp_gained,
        palace,
    }];
    cards.extend(
        summary
            .new_trophies
            .iter()
            .map(|id| CardKind::achievement(id, Some(crate::share::today()))),
    );
    if let Some(week) = week.filter(|week| week.target > 0 && week.days >= week.target) {
        cards.push(CardKind::Week {
            days: week.days,
            streak: week.streak,
        });
    }
    cards
}

#[component]
fn Done(
    summary: Summary,
    screen: RwSignal<Screen>,
    art: RwSignal<Option<UiAssets>>,
    /// The palace of the walk, when it asked in one only.
    palace: Option<String>,
    week: Option<Week>,
) -> impl IntoView {
    let asked = summary.asked;
    let lines = summary_lines(&summary);
    let cards = StoredValue::new(if asked > 0 {
        done_cards(&summary, palace, week.as_ref())
    } else {
        Vec::new()
    });
    view! {
        <section class="card panel journey-done" role="status">
            {move || {
                art.get()
                    .filter(|_| asked > 0)
                    .map(|a| view! { <div class="done-art"><Sply media=a.done /></div> })
            }}
            {if summary.asked == 0 {
                view! { <div class="prompt">{t("Nichts zu üben – alles erledigt.")}</div> }.into_any()
            } else {
                view! {
                    <div class="prompt">
                        {match summary.xp_gained {
                            Some(xp) => crate::i18n::tf(
                                "{0} von {1} gewusst · +{2} XP",
                                &[&summary.correct, &summary.asked, &xp],
                            ),
                            None if summary.points == 1 => crate::i18n::tf(
                                "{0} von {1} gewusst · 1 Punkt",
                                &[&summary.correct, &summary.asked],
                            ),
                            None => crate::i18n::tf(
                                "{0} von {1} gewusst · {2} Punkte",
                                &[&summary.correct, &summary.asked, &summary.points],
                            ),
                        }}
                    </div>
                }
                    .into_any()
            }}
            {(!lines.is_empty()).then(|| view! {
                <ul class="done-lines">
                    {lines.into_iter().map(|line| view! { <li>{line}</li> }).collect_view()}
                </ul>
            })}
            {summary
                .notes
                .into_iter()
                .map(|n| view! { <p class="notes">{crate::i18n::translate_message(&n)}</p> })
                .collect_view()}
            <div class="done-actions">
                <button class="primary" on:click=move |_| screen.set(crate::start_screen())>
                    {t("Zur Übersicht")}
                </button>
                {(asked > 0).then(|| view! {
                    <button type="button" on:click=move |event| {
                        crate::share::open(cards.get_value(), crate::modal::opener_of(&event));
                    }>{t("Teilen…")}</button>
                })}
            </div>
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_app_api::{Media, MediaKind};

    #[test]
    fn the_answer_line_has_no_hearts_and_recall_shows_no_run() {
        // The texts are checked in German; the app starts in English.
        crate::i18n::set_current(crate::i18n::Lang::De);
        assert_eq!(
            game_line(GameView { combo: Some(3) }, true).as_deref(),
            Some("3 in Folge")
        );
        assert_eq!(game_line(GameView { combo: None }, true), None, "recall");
        assert_eq!(game_line(GameView { combo: Some(0) }, true), None);
        assert_eq!(
            game_line(GameView { combo: Some(1) }, true),
            None,
            "a run starts at two"
        );
        let missed = game_line(GameView { combo: None }, false).unwrap();
        assert_eq!(missed, "Kommt bald wieder – so bleibt es hängen.");
        assert!(!missed.contains('♥') && !missed.contains("Herz"));
    }

    #[test]
    fn the_week_never_speaks_of_loss() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let week = |streak, best, missed_last| Week {
            days: 2,
            goal: 4,
            free: 0,
            target: 4,
            streak,
            best,
            missed_last,
        };
        assert_eq!(week_run_text(&week(0, 0, false)), None);
        assert_eq!(
            week_run_text(&week(1, 1, false)).unwrap(),
            "Wochenziel erreicht"
        );
        assert_eq!(
            week_run_text(&week(3, 3, true)).unwrap(),
            "Wochenziel 3 Wochen in Folge erreicht"
        );
        let after = week_run_text(&week(0, 5, true)).unwrap();
        assert_eq!(
            after,
            "Neue Woche, neues Ziel. Dein Bestwert: 5 Wochen in Folge."
        );
        for lang in crate::i18n::Lang::ALL {
            crate::i18n::set_current(lang);
            let texts = [
                week_run_text(&week(0, 5, true)).unwrap(),
                week_text(&week(0, 5, true)),
                today_text(&Today {
                    due_done: 0,
                    due_open: 0,
                    new_done: 0,
                    repeat_open: 0,
                    goal: Goal::Free,
                }),
            ];
            for text in texts {
                let lower = text.to_lowercase();
                for word in ["verlor", "lost", "perdu", "失", "streak"] {
                    assert!(!lower.contains(word), "{text}");
                }
            }
        }
        crate::i18n::set_current(crate::i18n::Lang::De);
    }

    #[test]
    fn the_week_counts_days_with_nothing_due_and_reads_in_the_singular() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let week = Week {
            days: 1,
            goal: 4,
            free: 2,
            target: 2,
            streak: 0,
            best: 1,
            missed_last: true,
        };
        assert_eq!(
            week_text(&week),
            "Diese Woche: 1 von 2 Lerntagen · 2 Tage ohne Fälliges"
        );
        assert_eq!(
            week_run_text(&week).unwrap(),
            "Neue Woche, neues Ziel. Dein Bestwert: 1 Woche in Folge."
        );
        let beyond = Week {
            days: 5,
            free: 0,
            target: 4,
            ..week.clone()
        };
        assert_eq!(week_text(&beyond), "Diese Woche: 4 von 4 Lerntagen");
        crate::i18n::set_current(crate::i18n::Lang::Fr);
        let one = Week {
            days: 1,
            free: 0,
            target: 4,
            ..week
        };
        assert_eq!(
            week_text(&one),
            "Cette semaine\u{202f}: 1 jour d’apprentissage sur 4"
        );
        crate::i18n::set_current(crate::i18n::Lang::De);
    }

    #[test]
    fn the_end_card_shows_points_only_when_a_plugin_changed_them() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let summary = Summary {
            asked: 3,
            correct: 2,
            points: 2,
            xp_gained: Some(26),
            due_done: 2,
            new_done: 1,
            due_left_today: Some(0),
            next_due: Some(("2026-10-21".into(), 6)),
            next_due_in: Some(1),
            new_trophies: vec!["game-first-journey".into()],
            ..Summary::default()
        };
        let lines = summary_lines(&summary);
        assert!(
            !lines.iter().any(|line| line.starts_with("Punkte")),
            "{lines:?}"
        );
        assert_eq!(
            lines,
            [
                "2 fällige Wiederholungen · 1 neuer Ort",
                "Alle fälligen Wiederholungen für heute erledigt",
                "Neue Trophäe: Erste Journey",
                "Nächste Wiederholung: morgen · 6 Orte",
            ]
        );
        let scored = Summary {
            points: 5,
            ..summary.clone()
        };
        assert_eq!(summary_lines(&scored)[0], "Punkte: 5");
        assert!(
            summary_lines(&Summary::default()).is_empty(),
            "no plugin, no lines"
        );
    }

    #[test]
    fn the_end_card_says_the_early_loci_own_date_and_what_comes_back_today() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        // A new learner: two loci forgotten, nothing was due.
        let first = Summary {
            asked: 9,
            correct: 7,
            points: 7,
            xp_gained: Some(54),
            due_done: 0,
            new_done: 9,
            due_left_today: Some(0),
            goal: Some(Goal::Free),
            repeat_later: 2,
            next_due: Some(("2026-10-24".into(), 7)),
            next_due_in: Some(1),
            new_trophies: vec!["palace-7@Küche".into()],
            ..Summary::default()
        };
        assert_eq!(
            summary_lines(&first),
            [
                "0 fällige Wiederholungen · 9 neue Orte",
                "Heute ist nichts fällig",
                "Kommt heute noch einmal zum Üben (ohne XP): 2 Orte",
                "Neue Medaille: Verankert · Küche",
                "Nächste Wiederholung: morgen · 7 Orte",
            ]
        );
        // Practised early: their own next review, not the palace's.
        let early = Summary {
            asked: 8,
            correct: 8,
            points: 8,
            xp_gained: Some(10),
            due_done: 1,
            new_done: 0,
            early: 7,
            repeat: 1,
            due_left_today: Some(0),
            goal: Some(Goal::AllDone),
            early_next: Some(("2026-11-02".into(), 10)),
            next_due: Some(("2026-10-24".into(), 1)),
            next_due_in: Some(1),
            ..Summary::default()
        };
        let lines = summary_lines(&early);
        assert_eq!(lines[0], "1 fällige Wiederholung · 0 neue Orte");
        assert_eq!(
            lines[1],
            "7 Orte waren noch nicht fällig und brachten deshalb keine XP. Ihre nächste Wiederholung: 2026-11-02."
        );
        assert_eq!(
            lines[2],
            "1 Ort hattest du heute schon; noch einmal üben bringt keine XP."
        );
        crate::i18n::set_current(crate::i18n::Lang::En);
        assert_eq!(
            done_counts(1, 1),
            "1 due review · 1 new locus",
            "singular in English too"
        );
        crate::i18n::set_current(crate::i18n::Lang::De);
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
            audio: None,
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
            <div class="prompt">{t("Eingaben für diese Journey")}</div>
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
            <button class="primary" type="submit">{t("Los ▶")}</button>
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
