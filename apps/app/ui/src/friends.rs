//! „Freunde & Ranglisten“: the friends of the Locora Cloud account, the requests to it and those it
//! sent, invite links, the rankings and reporting a name. The backend answers with `Social` states;
//! this page words each of them itself (`calm`), so no server message about access or routes shows.
//!
//! Names come from other people: they are made safe in the core and shown here as text only, in
//! `<bdi>`. No address of another person is ever shown, and a typed address is cleared once it was
//! sent. Each part of the page has its own status line right after it; the focus goes there after
//! an action, because the row whose button had it may be gone. Reloads keep the page on screen.

use std::cell::Cell;
use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;
use locora_app_api::{
    CMD_CLOUD_FRIEND_ASK, CMD_CLOUD_FRIEND_REMOVE, CMD_CLOUD_FRIENDS, CMD_CLOUD_INVITE_ACCEPT,
    CMD_CLOUD_INVITE_CREATE, CMD_CLOUD_INVITE_PREVIEW, CMD_CLOUD_INVITE_REVOKE, CMD_CLOUD_RANKING,
    CMD_CLOUD_REPORT, CMD_SHARE_SEND, FriendAskArgs, FriendAsked, FriendEntry, FriendIdArgs,
    FriendsList, InviteArgs, InviteFrom, InviteLink, InviteList, RankingArgs, RankingList,
    RankingRow, ReportArgs, Reported, SentRequest, ShareDone, ShareSendArgs, Social,
};

use crate::i18n::{t, tf, translate_message};
use crate::info::Info;
use crate::modal::{Modal, opener_of};
use crate::tauri::{NoArgs, call};
use crate::{Screen, start_screen};

/// What an action did, for the status line of its part of the page.
#[derive(Clone, Debug, PartialEq)]
struct Note {
    text: String,
    alert: bool,
    /// The line takes the focus (the button that was pressed may be gone).
    focus: bool,
}

impl Note {
    fn said(text: impl Into<String>) -> Option<Self> {
        Some(Self {
            text: text.into(),
            alert: false,
            focus: true,
        })
    }

    fn failed(text: impl Into<String>) -> Option<Self> {
        Some(Self {
            text: text.into(),
            alert: true,
            focus: true,
        })
    }
}

type Notes = RwSignal<Option<Note>>;

/// Where a `Social` state that is not a result is worded: each part says it in its own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    Page,
    Ask,
    Remove,
    Ranking,
    Report,
    Links,
    Accept,
}

const SIGNED_OUT: &str = "Freunde, Einladungen und Ranglisten gehören zu Locora Cloud. Melde dich in den Einstellungen unter „Locora Cloud“ an, dann siehst du sie hier.";
const SIGN_IN_AGAIN: &str = "Deine Anmeldung bei Locora Cloud ist abgelaufen. Melde dich in den Einstellungen unter „Locora Cloud“ auf diesem Gerät ab und wieder an.";

/// The words for a state without a result, `None` for a result. Neutral: none of them says how to
/// get access.
fn calm<T>(state: &Social<T>, context: Context) -> Option<&'static str> {
    Some(match state {
        Social::Ready(_) => return None,
        Social::SignedOut => t(SIGNED_OUT),
        Social::SignInAgain => t(SIGN_IN_AGAIN),
        Social::NotForAccount => match context {
            Context::Ask => t("Freundschaftsanfragen sind für dieses Konto nicht verfügbar."),
            Context::Links => t("Einladungslinks sind für dieses Konto nicht verfügbar."),
            Context::Accept => t("Einladungen annehmen ist für dieses Konto nicht verfügbar."),
            Context::Ranking => t("Ranglisten sind für dieses Konto nicht verfügbar."),
            Context::Page | Context::Remove | Context::Report => {
                t("Freunde, Einladungen und Ranglisten sind für dieses Konto nicht verfügbar.")
            }
        },
        Social::ServerTooOld => match context {
            Context::Page => t(
                "Dieser Server kennt Freunde und Ranglisten noch nicht. Sobald er aktualisiert ist, findest du sie hier.",
            ),
            Context::Ask => t(
                "Dieser Server kennt noch keine Freundschaftsanfragen. Das geht, sobald er aktualisiert ist.",
            ),
            Context::Remove => t(
                "Dieser Server kann Freundschaften und Anfragen noch nicht entfernen. Das geht, sobald er aktualisiert ist.",
            ),
            Context::Ranking => t(
                "Dieser Server kennt noch keine Ranglisten. Sobald er aktualisiert ist, findest du sie hier.",
            ),
            Context::Report => t(
                "Dieser Server nimmt noch keine Meldungen von Namen an. Das geht, sobald er aktualisiert ist.",
            ),
            Context::Links | Context::Accept => t(
                "Dieser Server kennt noch keine Einladungslinks. Das geht, sobald er aktualisiert ist.",
            ),
        },
    })
}

/// „1 Ranglistenpunkt“, „80 Ranglistenpunkte“.
fn points_text(points: u64) -> String {
    if points == 1 {
        t("1 Ranglistenpunkt").to_owned()
    } else {
        tf("{0} Ranglistenpunkte", &[&points])
    }
}

/// A moment (Unix seconds) in the current language, with the day only or with the time too.
fn when(secs: u64, time: bool) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(secs as f64 * 1000.0));
    let options = js_sys::Object::new();
    let set = |key: &str, value: &str| {
        let _ = js_sys::Reflect::set(&options, &key.into(), &value.into());
    };
    set("year", "numeric");
    set("month", "long");
    set("day", "numeric");
    if time {
        set("hour", "2-digit");
        set("minute", "2-digit");
    }
    let lang = crate::i18n::current().code();
    if time {
        date.to_locale_string(lang, &options).into()
    } else {
        date.to_locale_date_string(lang, &options).into()
    }
}

thread_local! {
    static NEXT_ID: Cell<u32> = const { Cell::new(0) };
}

/// An id for an element of a row (a hint its field names), unique on the page.
fn unique_id(prefix: &str) -> String {
    let id = NEXT_ID.with(|next| {
        let id = next.get().wrapping_add(1);
        next.set(id);
        id
    });
    format!("{prefix}-{id}")
}

/// A name to report, with the line that will say it was reported.
#[derive(Clone)]
struct ReportTarget {
    name: String,
    reference: String,
    note: Notes,
}

/// What the parts of the page share.
#[derive(Clone, Copy)]
struct Page {
    list: RwSignal<Option<Result<Social<FriendsList>, String>>>,
    reload: Callback<()>,
    /// After asking someone: the requests in „Deine Anfragen“ before the request was sent. One
    /// that was not among them says it was just sent; the server often makes no new one (the
    /// person was asked already), and then none does.
    sent_before: RwSignal<Option<HashSet<String>>>,
    /// Bumped after a change of friends; the friends ranking reads itself again.
    friends_changed: RwSignal<u32>,
    /// The invite link just made, until it is closed or the page left.
    new_link: RwSignal<Option<InviteLink>>,
    /// While a new invite link is made: a second tap makes no second one.
    creating: RwSignal<bool>,
    /// „Einladungslink erstellen“, which gets the focus back when the new link's box closes.
    create_button: NodeRef<leptos::html::Button>,
    /// The status line under the new link.
    link_note: Notes,
    report: RwSignal<Option<ReportTarget>>,
    report_opener: StoredValue<Option<web_sys::HtmlElement>, LocalStorage>,
}

impl Page {
    /// Reads the list again.
    fn refresh(&self) {
        self.sent_before.set(None);
        self.reload.run(());
    }

    /// The ids of the requests this account sent, as the page shows them (before asking).
    fn sent_ids(&self) -> HashSet<String> {
        self.with(|list| {
            list.sent
                .iter()
                .filter_map(|sent| sent.id.clone())
                .collect()
        })
        .unwrap_or_default()
    }

    /// Reads the list again after asking; a request not in `before` is marked as just sent.
    fn refresh_asked(&self, before: HashSet<String>) {
        self.sent_before.set(Some(before));
        self.reload.run(());
    }

    fn friends_changed(&self) {
        self.friends_changed.update(|n| *n = n.wrapping_add(1));
    }

    fn with<R>(&self, read: impl FnOnce(&FriendsList) -> R) -> Option<R> {
        self.list.with(|page| match page {
            Some(Ok(Social::Ready(list))) => Some(read(list)),
            _ => None,
        })
    }

    /// Opens the report dialog for `name`, from the button of `event`.
    fn report(&self, event: &web_sys::MouseEvent, name: String, reference: String, note: Notes) {
        self.report_opener.set_value(opener_of(event));
        self.report.set(Some(ReportTarget {
            name,
            reference,
            note,
        }));
    }

    /// Makes a new invite link and shows it under „Freunde einladen“; not while one is made.
    fn create_link(&self) {
        if self.creating.get_untracked() {
            return;
        }
        self.creating.set(true);
        let page = *self;
        spawn_local(async move {
            let note = match call::<Social<InviteLink>>(CMD_CLOUD_INVITE_CREATE, NoArgs {}).await {
                Ok(Social::Ready(link)) => {
                    let said = tf(
                        "Einladungslink erstellt. Er gilt einmal, bis {0}. Unter „Offene Einladungslinks“ kannst du ihn zurückziehen.",
                        &[&when(link.expires_at, false)],
                    );
                    page.new_link.try_set(Some(link));
                    page.refresh();
                    Note::said(said)
                }
                Ok(state) => Note::failed(calm(&state, Context::Links).unwrap_or_default()),
                Err(error) => Note::failed(error),
            };
            page.link_note.try_set(note);
            page.creating.try_set(false);
        });
    }
}

/// The line that says what an action did. A new note gets a new paragraph inside the live
/// region, so the same words twice are announced twice; it takes the focus when the note asks.
#[component]
fn Said(note: Notes) -> impl IntoView {
    let line = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        let focus = note.with(|note| note.as_ref().is_some_and(|note| note.focus));
        if let (true, Some(line)) = (focus, line.get())
            && line.is_connected()
        {
            let _ = line.focus();
        }
    });
    view! {
        <div class="friends-said" aria-live="polite">
            {move || note.get().map(|note| view! {
                <p tabindex="-1" node_ref=line class=if note.alert { "notes" } else { "status" }
                    role=note.alert.then_some("alert")>{note.text}</p>
            })}
        </div>
    }
}

/// The friends page.
#[component]
pub fn FriendsScreen(screen: RwSignal<Screen>, from_settings: bool) -> impl IntoView {
    let list = RwSignal::new(None::<Result<Social<FriendsList>, String>>);
    let reloading = RwSignal::new(false);
    // A reload that failed while the page shows a list: the list stays, this says why it is old.
    let stale = RwSignal::new(None::<String>);
    let generation = StoredValue::new(0_u32);
    let reload = Callback::new(move |()| {
        let request = generation.get_value().wrapping_add(1);
        generation.set_value(request);
        reloading.set(true);
        spawn_local(async move {
            let result = call::<Social<FriendsList>>(CMD_CLOUD_FRIENDS, NoArgs {}).await;
            if generation.try_get_value() != Some(request) {
                return;
            }
            let shown = list
                .try_with_untracked(|page| matches!(page, Some(Ok(Social::Ready(_)))))
                .unwrap_or(false);
            match result {
                Err(error) if shown => {
                    stale.try_set(Some(error));
                }
                result => {
                    stale.try_set(None);
                    list.try_set(Some(result));
                }
            }
            reloading.try_set(false);
        });
    });
    let page = Page {
        list,
        reload,
        sent_before: RwSignal::new(None),
        friends_changed: RwSignal::new(0),
        new_link: RwSignal::new(None),
        creating: RwSignal::new(false),
        create_button: NodeRef::new(),
        link_note: RwSignal::new(None),
        report: RwSignal::new(None),
        report_opener: StoredValue::new_local(None),
    };
    provide_context(page);
    reload.run(());
    // Only a change of what is shown rebuilds the page; a reload of the same kind keeps it.
    #[derive(Clone, Copy, PartialEq)]
    enum Shown {
        Loading,
        Failed,
        Calm(Option<&'static str>, bool),
        Ready,
        LinksOnly,
    }
    let shown = Memo::new(move |_| {
        list.with(|page| match page {
            None => Shown::Loading,
            Some(Err(_)) => Shown::Failed,
            Some(Ok(Social::Ready(list))) if list.access => Shown::Ready,
            Some(Ok(Social::Ready(_))) => Shown::LinksOnly,
            Some(Ok(state)) => Shown::Calm(
                calm(state, Context::Page),
                matches!(state, Social::SignedOut | Social::SignInAgain),
            ),
        })
    });
    let auto = crate::auto_sync::use_auto_sync();
    let waiting = move || {
        crate::tauri::in_browser() && auto.is_some_and(|auto| auto.view.with(|view| view.running))
    };
    // Back in the settings, the button that led here has the focus again, in view.
    let back = move |_| {
        if from_settings {
            crate::cloud_settings::open_cloud_at(
                screen,
                crate::cloud_settings::CloudFocus::Friends,
            );
        } else {
            screen.set(start_screen());
        }
    };
    let heading = NodeRef::<leptos::html::H1>::new();
    // A retry shows the page loading again, so that the same error is announced again; the focus
    // waits on the heading meanwhile (the button goes).
    let retry = move || {
        if let Some(heading) = heading.get_untracked() {
            let _ = heading.focus();
        }
        list.set(None);
        reload.run(());
    };
    view! {
        <header class="bar friends-bar">
            <button class="back" title=t("Zurück") aria-label=t("Zurück") on:click=back>"←"</button>
            <h1 tabindex="-1" node_ref=heading>{t("Freunde & Ranglisten")}</h1>
        </header>
        {move || stale.get().map(|error| view! { <p class="notes" role="alert">{error}</p> })}
        {move || match shown.get() {
            Shown::Loading => if waiting() {
                view! { <p class="progress" role="status">{t("Wartet auf den laufenden Sync …")}</p> }.into_any()
            } else {
                view! { <crate::Loading /> }.into_any()
            },
            Shown::Failed => view! {
                <section class="card friends-calm">
                    <p class="notes" role="alert">{move || list.with(|page| match page {
                        Some(Err(error)) => error.clone(),
                        _ => String::new(),
                    })}</p>
                    <button type="button" on:click=move |_| retry()>{t("Erneut versuchen")}</button>
                </section>
            }.into_any(),
            Shown::Calm(text, settings) => view! {
                <section class="card friends-calm" role="status">
                    <p>{text.unwrap_or_default()}</p>
                    {settings.then(|| view! {
                        <button type="button" class="primary" on:click=move |_| crate::cloud_settings::open_cloud(screen)>
                            {t("Einstellungen öffnen")}
                        </button>
                    })}
                </section>
            }.into_any(),
            Shown::LinksOnly => view! {
                <section class="card friends-calm">
                    <p role="status">{t("Freunde, Einladungen und Ranglisten sind für dieses Konto nicht verfügbar.")}</p>
                    <OpenLinks access=false />
                </section>
            }.into_any(),
            Shown::Ready => view! {
                <div class="friends-page" aria-busy=move || reloading.get().then_some("true")>
                    <div class="friends-column">
                        <FriendsCard />
                        <InviteCard />
                    </div>
                    <div class="friends-column">
                        <RankingCard />
                    </div>
                </div>
            }.into_any(),
        }}
        {move || page.report.get().map(|target| view! { <ReportDialog target=target /> })}
    }
}

/// A key for a row: its id, or (an older server sends none) its place and name.
fn keyed(entries: &[FriendEntry]) -> Vec<(String, FriendEntry)> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let key = match &entry.id {
                Some(id) => format!("id:{id}"),
                None => format!("{index}:{}", entry.name),
            };
            (key, entry.clone())
        })
        .collect()
}

/// „Freunde“: requests to this account, the friends, the requests it sent.
#[component]
fn FriendsCard() -> impl IntoView {
    let page = expect_context::<Page>();
    let incoming_note = RwSignal::new(None);
    let friends_note = RwSignal::new(None);
    let sent_note = RwSignal::new(None);
    let incoming = move || page.with(|list| keyed(&list.incoming)).unwrap_or_default();
    let friends = move || page.with(|list| keyed(&list.friends)).unwrap_or_default();
    let sent = move || page.with(|list| list.sent.clone()).unwrap_or_default();
    let empty = move || {
        page.with(|list| {
            list.incoming.is_empty()
                && list.friends.is_empty()
                && list.sent.is_empty()
                && list.open == 0
        })
        .unwrap_or(false)
    };
    // An older server sends no ids: its entries cannot be answered from here.
    let without_ids = move || {
        page.with(|list| {
            list.incoming
                .iter()
                .chain(&list.friends)
                .any(|entry| entry.id.is_none())
        })
        .unwrap_or(false)
    };
    let open = move || page.with(|list| list.open).unwrap_or(0);
    let has_open = move || open() > 0;
    let sent_rows = move || {
        sent()
            .into_iter()
            .enumerate()
            .map(|(index, request)| {
                let key = match &request.id {
                    Some(id) => format!("id:{id}"),
                    None => format!("{index}"),
                };
                (key, request)
            })
            .collect::<Vec<_>>()
    };
    view! {
        <section class="card friends-card" aria-labelledby="friends-title">
            <h2 id="friends-title">{t("Freunde")}</h2>
            <Show when=empty>
                <p class="notes">{t("Noch keine Freunde. Lade jemanden mit einem Link ein oder schick eine Anfrage an die E-Mail-Adresse der Person.")}</p>
            </Show>
            // A list only where there are rows: an empty one is noise for a screen reader.
            <Show when=move || !incoming().is_empty()>
                <h3>{t("Anfragen an dich")}</h3>
                <ul class="friend-rows">
                    <For each=incoming key=|(key, _)| key.clone() children=move |(_, entry)| view! {
                        <IncomingRow entry=entry note=incoming_note />
                    } />
                </ul>
            </Show>
            <Said note=incoming_note />
            <Show when=move || !friends().is_empty()>
                <h3>{t("Deine Freunde")}</h3>
                <ul class="friend-rows">
                    <For each=friends key=|(key, _)| key.clone() children=move |(_, entry)| view! {
                        <FriendRow entry=entry note=friends_note />
                    } />
                </ul>
            </Show>
            <Said note=friends_note />
            <Show when=move || !sent().is_empty() || has_open()>
                <h3>{t("Deine Anfragen")}</h3>
            </Show>
            <Show when=move || !sent().is_empty()>
                <ul class="friend-rows">
                    <For each=sent_rows key=|(key, _)| key.clone() children=move |(_, request)| {
                        let id = request.id.clone();
                        let fresh = move || {
                            page.sent_before.with(|before| {
                                before.as_ref().zip(id.as_ref()).is_some_and(|(before, id)| !before.contains(id))
                            })
                        };
                        view! { <SentRow sent=request fresh=Signal::derive(fresh) note=sent_note /> }
                    } />
                </ul>
            </Show>
            <Show when=has_open>
                <p>{move || tf("Offene Anfragen: {0}", &[&open()])}</p>
            </Show>
            <Show when=move || !sent().is_empty()>
                <p class="notes">{t("Wen du angefragt hast, zeigt Locora erst, wenn die Person annimmt.")}</p>
            </Show>
            <Said note=sent_note />
            <Show when=without_ids>
                <p class="notes">{t("Dieser Server kann Freundschaften und Anfragen noch nicht entfernen. Das geht, sobald er aktualisiert ist.")}</p>
            </Show>
        </section>
    }
}

/// Ends a friendship, withdraws a request or declines one, and says so in `note`.
fn remove(page: Page, id: String, done: String, note: Notes, busy: RwSignal<bool>) {
    busy.set(true);
    spawn_local(async move {
        let said = match call::<Social<()>>(CMD_CLOUD_FRIEND_REMOVE, FriendIdArgs { id }).await {
            Ok(Social::Ready(())) => {
                page.refresh();
                page.friends_changed();
                Note::said(done)
            }
            Ok(state) => Note::failed(calm(&state, Context::Remove).unwrap_or_default()),
            Err(error) => {
                // A friendship or request that is gone already: the list shows what is left.
                page.refresh();
                Note::failed(error)
            }
        };
        note.try_set(said);
        busy.try_set(false);
    });
}

/// A confirmation under a row: the question takes the focus; Escape and „Abbrechen“ close it and
/// give the focus back to the button that opened it.
#[component]
fn Confirm(
    question: String,
    /// The button that does it (a catalog text).
    yes: &'static str,
    busy: RwSignal<bool>,
    on_yes: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let warning = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        if let Some(warning) = warning.get() {
            let _ = warning.focus();
        }
    });
    view! {
        <div class="friend-confirm" on:keydown=move |event| {
            if event.key() == "Escape" {
                event.prevent_default();
                on_cancel.run(());
            }
        }>
            <p role="alert" tabindex="-1" node_ref=warning>{question}</p>
            <div class="friend-actions">
                <button type="button" class="danger" disabled=move || busy.get() on:click=move |_| on_yes.run(())>{yes}</button>
                <button type="button" disabled=move || busy.get() on:click=move |_| on_cancel.run(())>{t("Abbrechen")}</button>
            </div>
        </div>
    }
}

/// A request to this account: accept it by asking back with the person's address, decline it,
/// report the name.
#[component]
fn IncomingRow(entry: FriendEntry, note: Notes) -> impl IntoView {
    let page = expect_context::<Page>();
    let name = StoredValue::new(entry.name.clone());
    let id = StoredValue::new(entry.id.clone());
    let asking = RwSignal::new(false);
    let declining = RwSignal::new(false);
    let busy = RwSignal::new(false);
    // After an answer that did not make friends: an invite link is the easier way.
    let offer = RwSignal::new(false);
    let email = RwSignal::new(String::new());
    let form_note = RwSignal::new(None::<Note>);
    let accept_button = NodeRef::<leptos::html::Button>::new();
    let decline_button = NodeRef::<leptos::html::Button>::new();
    let field = NodeRef::<leptos::html::Input>::new();
    // The field names the hint after it, which says what a wrong address does.
    let hint = unique_id("ask-back-hint");
    Effect::new(move |_| {
        if let Some(field) = field.get() {
            let _ = field.focus();
        }
    });
    let close_form = move || {
        asking.set(false);
        form_note.set(None);
        if let Some(button) = accept_button.get_untracked() {
            let _ = button.focus();
        }
    };
    let ask_back = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        form_note.set(None);
        let address = email.get_untracked();
        let before = page.sent_ids();
        spawn_local(async move {
            let name = name.get_value();
            match call::<Social<FriendAsked>>(
                CMD_CLOUD_FRIEND_ASK,
                FriendAskArgs { email: address },
            )
            .await
            {
                Ok(Social::Ready(asked)) => {
                    email.try_set(String::new());
                    asking.try_set(false);
                    if asked.accepted {
                        page.friends_changed();
                        page.refresh();
                        note.try_set(Note::said(tf(
                            "Du bist mit {0} befreundet.",
                            &[&asked.name.unwrap_or(name)],
                        )));
                    } else {
                        offer.try_set(true);
                        page.refresh_asked(before);
                        note.try_set(Note::said(tf(
                            "Noch nicht befreundet. Eine Anfrage an diese Adresse wartet jetzt. Gehört die Adresse {0}, seid ihr befreundet, sobald sie bestätigt ist. Hast du dich vertippt, kannst du die Anfrage unter „Deine Anfragen“ zurückziehen.",
                            &[&name],
                        )));
                    }
                }
                Ok(state) => {
                    form_note.try_set(Note::failed(calm(&state, Context::Ask).unwrap_or_default()));
                }
                Err(error) => {
                    form_note.try_set(Note::failed(error));
                }
            }
            busy.try_set(false);
        });
    };
    let decline = move || {
        let Some(id) = id.get_value() else { return };
        let done = tf("Anfrage von {0} abgelehnt.", &[&name.get_value()]);
        declining.set(false);
        remove(page, id, done, note, busy);
    };
    let cancel_decline = move || {
        declining.set(false);
        if let Some(button) = decline_button.get_untracked() {
            let _ = button.focus();
        }
    };
    let link_instead = move || {
        asking.set(false);
        page.create_link();
    };
    view! {
        <li class="friend-row">
            <div class="friend-line">
                <bdi class="friend-name">{entry.name.clone()}</bdi>
                <div class="friend-actions">
                    <button type="button" node_ref=accept_button disabled=move || busy.get()
                        aria-expanded=move || if asking.get() { "true" } else { "false" }
                        aria-label=tf("Anfrage von {0} annehmen", &[&entry.name])
                        on:click=move |_| {
                            declining.set(false);
                            asking.update(|open| *open = !*open);
                        }>{t("Annehmen…")}</button>
                    {entry.id.is_some().then(|| view! {
                        <button type="button" node_ref=decline_button disabled=move || busy.get()
                            aria-label=tf("Anfrage von {0} ablehnen", &[&name.get_value()])
                            on:click=move |_| {
                                asking.set(false);
                                declining.set(true);
                            }>{t("Ablehnen")}</button>
                        <button type="button" aria-label=tf("Namen „{0}“ melden", &[&name.get_value()])
                            on:click=move |event| {
                                if let Some(reference) = id.get_value() {
                                    page.report(&event, name.get_value(), reference, note);
                                }
                            }>{t("Melden")}</button>
                    })}
                    <Show when=move || offer.get() && !asking.get()>
                        <button type="button" disabled=move || page.creating.get()
                            on:click=move |_| link_instead()>{t("Stattdessen Einladungslink schicken…")}</button>
                    </Show>
                </div>
            </div>
            <Show when=move || asking.get()>
                // The app checks the address itself and says so in its own words and language.
                <form class="friend-form keep-in-view" novalidate on:keydown=move |event| {
                        if event.key() == "Escape" {
                            event.prevent_default();
                            close_form();
                        }
                    }
                    on:submit=move |event| {
                        event.prevent_default();
                        ask_back();
                    }>
                    <label class="input">
                        <span>{tf("E-Mail-Adresse von {0}", &[&name.get_value()])}</span>
                        <input type="email" inputmode="email" autocomplete="off" autocapitalize="none"
                            spellcheck="false" maxlength="254" node_ref=field bind:value=email
                            aria-describedby=hint.clone()
                            on:focus=move |event| crate::keep_in_view(event_target(&event)) />
                    </label>
                    <p class="notes" id=hint.clone()>{tf("Zum Annehmen fragst du {0} mit der Adresse an, mit der die Person bei Locora Cloud angemeldet ist. Locora zeigt Adressen anderer nie an. Ist es nicht die Adresse von {0}, geht eine neue Anfrage an diese Adresse.", &[&name.get_value()])}</p>
                    <div class="friend-actions">
                        <button type="submit" class="primary" disabled=move || busy.get()>{t("Zurück anfragen")}</button>
                        <button type="button" disabled=move || page.creating.get()
                            on:click=move |_| link_instead()>{t("Stattdessen Einladungslink schicken…")}</button>
                        <button type="button" on:click=move |_| close_form()>{t("Abbrechen")}</button>
                    </div>
                    <Said note=form_note />
                </form>
            </Show>
            <Show when=move || declining.get()>
                <Confirm
                    question=tf("Anfrage von {0} ablehnen? Neue Anfragen dieser Person kommen erst wieder an, wenn du sie selbst anfragst.", &[&name.get_value()])
                    yes=t("Ja, ablehnen")
                    busy=busy
                    on_yes=Callback::new(move |()| decline())
                    on_cancel=Callback::new(move |()| cancel_decline()) />
            </Show>
        </li>
    }
}

/// A friend: report the name, end the friendship.
#[component]
fn FriendRow(entry: FriendEntry, note: Notes) -> impl IntoView {
    let page = expect_context::<Page>();
    let name = StoredValue::new(entry.name.clone());
    let id = StoredValue::new(entry.id.clone());
    let ending = RwSignal::new(false);
    let busy = RwSignal::new(false);
    let remove_button = NodeRef::<leptos::html::Button>::new();
    let end = move || {
        let Some(id) = id.get_value() else { return };
        let done = tf("Freundschaft mit {0} beendet.", &[&name.get_value()]);
        ending.set(false);
        remove(page, id, done, note, busy);
    };
    let cancel = move || {
        ending.set(false);
        if let Some(button) = remove_button.get_untracked() {
            let _ = button.focus();
        }
    };
    view! {
        <li class="friend-row">
            <div class="friend-line">
                <bdi class="friend-name">{entry.name.clone()}</bdi>
                {entry.id.is_some().then(|| view! {
                    <div class="friend-actions">
                        <button type="button" aria-label=tf("Namen „{0}“ melden", &[&name.get_value()])
                            on:click=move |event| {
                                if let Some(reference) = id.get_value() {
                                    page.report(&event, name.get_value(), reference, note);
                                }
                            }>{t("Melden")}</button>
                        <button type="button" node_ref=remove_button disabled=move || busy.get()
                            aria-label=tf("{0} entfernen", &[&name.get_value()])
                            on:click=move |_| ending.set(true)>{t("Entfernen")}</button>
                    </div>
                })}
            </div>
            <Show when=move || ending.get()>
                <Confirm
                    question=tf("Freundschaft mit {0} beenden? Ihr seht euch dann nicht mehr in der Rangliste eurer Freunde.", &[&name.get_value()])
                    yes=t("Ja, beenden")
                    busy=busy
                    on_yes=Callback::new(move |()| end())
                    on_cancel=Callback::new(move |()| cancel()) />
            </Show>
        </li>
    }
}

/// A request this account sent: when, and withdrawing it (at once).
#[component]
fn SentRow(sent: SentRequest, fresh: Signal<bool>, note: Notes) -> impl IntoView {
    let page = expect_context::<Page>();
    let busy = RwSignal::new(false);
    let moment = sent.since.map(|since| when(since, true));
    let label = match &moment {
        Some(moment) => tf("Gesendet am {0}", &[moment]),
        None => t("Gesendet").to_owned(),
    };
    let (withdraw_label, done) = match &moment {
        Some(moment) => (
            tf("Anfrage vom {0} zurückziehen", &[moment]),
            tf("Anfrage vom {0} zurückgezogen.", &[moment]),
        ),
        None => (
            t("Anfrage zurückziehen").to_owned(),
            t("Anfrage zurückgezogen.").to_owned(),
        ),
    };
    let id = StoredValue::new(sent.id.clone());
    let done = StoredValue::new(done);
    view! {
        <li class="friend-row">
            <div class="friend-line">
                <span class="friend-name">
                    {label}
                    <Show when=move || fresh.get()>
                        " · "<strong>{t("gerade gesendet")}</strong>
                    </Show>
                </span>
                {sent.id.is_some().then(|| view! {
                    <div class="friend-actions">
                        <button type="button" disabled=move || busy.get() aria-label=withdraw_label
                            on:click=move |_| {
                                if let Some(id) = id.get_value() {
                                    remove(page, id, done.get_value(), note, busy);
                                }
                            }>{t("Zurückziehen")}</button>
                    </div>
                })}
            </div>
        </li>
    }
}

/// The text that goes with an invite link when it is shared or sent.
fn invite_text(link: &InviteLink) -> String {
    let until = when(link.expires_at, false);
    match &link.url {
        Some(url) => tf(
            "Lass uns in Locora Freunde sein! Öffne den Link, um meine Einladung anzunehmen (gilt bis {0}): {1}",
            &[&until, url],
        ),
        None => tf(
            "Lass uns in Locora Freunde sein! Gib meinen Einladungscode in Locora unter „Freunde & Ranglisten“ ein (gilt bis {0}): {1}",
            &[&until, &link.code],
        ),
    }
}

/// „Freunde einladen“: a new link, asking by address, the open links, accepting an invite.
#[component]
fn InviteCard() -> impl IntoView {
    let page = expect_context::<Page>();
    #[derive(Clone, Copy, PartialEq)]
    enum Links {
        Ready,
        TooOld,
        Failed,
    }
    let links = Memo::new(move |_| {
        page.with(|list| match &list.invites {
            InviteList::Ready(_) => Links::Ready,
            InviteList::ServerTooOld => Links::TooOld,
            InviteList::Failed(_) => Links::Failed,
        })
        .unwrap_or(Links::Ready)
    });
    // Rebuilt only when the server turns out to know no links, not when the list loads again.
    let too_old = Memo::new(move |_| links.get() == Links::TooOld);
    // The heading of the open links stays while they load again, and keeps the focus of a retry.
    let links_heading = NodeRef::<leptos::html::H3>::new();
    let retry = move || {
        if let Some(heading) = links_heading.get_untracked() {
            let _ = heading.focus();
        }
        page.refresh();
    };
    view! {
        <section class="card invite-card" aria-labelledby="invite-title">
            <h2 id="invite-title">{t("Freunde einladen")}</h2>
            {move || too_old.get().then(|| view! {
                <p class="notes">{t("Dieser Server kennt noch keine Einladungslinks. Das geht, sobald er aktualisiert ist.")}</p>
            })}
            {move || (!too_old.get()).then(|| view! {
                <p>{t("Ein Einladungslink gilt einmal und eine begrenzte Zeit. Schick ihn nur der Person, die du einladen möchtest: Wer ihn zuerst annimmt, ist mit dir befreundet, und wer ihn hat, sieht deinen Anzeigenamen. Eine E-Mail-Adresse braucht es nicht.")}</p>
                <button type="button" class="primary" node_ref=page.create_button disabled=move || page.creating.get()
                    on:click=move |_| {
                        page.link_note.set(None);
                        page.create_link();
                    }>{t("Einladungslink erstellen")}</button>
                <NewLink />
                <Said note=page.link_note />
            })}
            <h3>{t("Jemanden anfragen")}</h3>
            <AskForm />
            {move || (!too_old.get()).then(|| view! {
                <h3 tabindex="-1" node_ref=links_heading>{t("Offene Einladungslinks")}</h3>
            })}
            {move || match links.get() {
                Links::TooOld => None,
                // A new paragraph per answer: the same error after a retry is announced again.
                Links::Failed => Some(view! {
                    {move || page.with(|list| match &list.invites {
                        InviteList::Failed(error) => Some(translate_message(error)),
                        _ => None,
                    }).flatten().map(|error| view! { <p class="notes" role="alert">{error}</p> })}
                    <button type="button" on:click=move |_| retry()>{t("Erneut versuchen")}</button>
                    <h3>{t("Einladung annehmen")}</h3>
                    <AcceptForm />
                }.into_any()),
                Links::Ready => Some(view! {
                    <OpenLinks access=true />
                    <h3>{t("Einladung annehmen")}</h3>
                    <AcceptForm />
                }.into_any()),
            }}
        </section>
    }
}

/// The link just made: to share, copy or select by hand.
#[component]
fn NewLink() -> impl IntoView {
    let page = expect_context::<Page>();
    let ways = RwSignal::new(None);
    spawn_local(async move {
        ways.try_set(Some(crate::share::ways().await));
    });
    let note = page.link_note;
    // While the share sheet is open: a second tap would be refused by the browser and say that
    // sharing is not possible here.
    let sharing = RwSignal::new(false);
    move || {
        page.new_link.get().map(|link| {
            let text = invite_text(&link);
            let shown = link.url.clone().unwrap_or_else(|| link.code.clone());
            let has_url = link.url.is_some();
            let copy_text = StoredValue::new(shown.clone());
            let share_text = StoredValue::new(text);
            let copy = move |_| {
                let copied = crate::share::copy_text(&copy_text.get_value());
                spawn_local(async move {
                    note.try_set(if copied.await {
                        Note::said(t("Kopiert."))
                    } else {
                        Note::failed(t("Kopieren nicht möglich. Bitte den Text oben markieren und kopieren."))
                    });
                });
            };
            let share = move |_| {
                if sharing.get_untracked() {
                    return;
                }
                sharing.set(true);
                let args = ShareSendArgs {
                    name: String::new(),
                    png: None,
                    text: share_text.get_value(),
                    title: t("Einladung zu Locora").to_owned(),
                };
                spawn_local(async move {
                    match call::<ShareDone>(CMD_SHARE_SEND, args).await {
                        Ok(ShareDone::Shared) => {
                            note.try_set(Note::said(t("Geteilt.")));
                        }
                        Ok(ShareDone::Cancelled) => {}
                        Ok(_) => {
                            note.try_set(Note::failed(t("Teilen ist hier nicht möglich. Bitte „Link kopieren“ verwenden.")));
                        }
                        Err(error) => {
                            note.try_set(Note::failed(error));
                        }
                    }
                    sharing.try_set(false);
                });
            };
            view! {
                <div class="invite-box" role="group" aria-label=t("Einladungslink")>
                    <input type="text" class="invite-url" readonly prop:value=shown
                        aria-label=if has_url { t("Einladungslink") } else { t("Einladungscode") }
                        on:focus=move |event| {
                            use wasm_bindgen::JsCast;
                            if let Ok(input) = event_target::<web_sys::EventTarget>(&event).dyn_into::<web_sys::HtmlInputElement>() {
                                input.select();
                            }
                        } />
                    <p class="invite-code">{tf("Code: {0}", &[&link.code])}</p>
                    {(!has_url).then(|| view! {
                        <p class="notes">{t("Die Person gibt den Code in Locora unter „Freunde & Ranglisten“ bei „Einladung annehmen“ ein.")}</p>
                    })}
                    <div class="friend-actions">
                        {move || ways.get().is_some_and(|ways: locora_app_api::ShareWays| ways.text_sheet).then(|| view! {
                            // Not `disabled`: cancelling the sheet says nothing, so the focus stays here.
                            <button type="button" aria-disabled=move || sharing.get().then_some("true")
                                on:click=share>{t("Teilen…")}</button>
                        })}
                        <button type="button" on:click=copy>{if has_url { t("Link kopieren") } else { t("Code kopieren") }}</button>
                        <button type="button" on:click=move |_| {
                            page.new_link.set(None);
                            note.set(None);
                            // The box goes: the focus goes back to the button that made the link.
                            if let Some(button) = page.create_button.get_untracked() {
                                let _ = button.focus();
                            }
                        }>{t("Schließen")}</button>
                    </div>
                </div>
            }
        })
    }
}

/// Asking someone by e-mail address. The address is cleared after every answer that is not an
/// error, and never shown again.
#[component]
fn AskForm() -> impl IntoView {
    let page = expect_context::<Page>();
    let email = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let note = RwSignal::new(None);
    let send = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        note.set(None);
        let address = email.get_untracked();
        let before = page.sent_ids();
        spawn_local(async move {
            let said = match call::<Social<FriendAsked>>(
                CMD_CLOUD_FRIEND_ASK,
                FriendAskArgs { email: address },
            )
            .await
            {
                Ok(Social::Ready(asked)) => {
                    email.try_set(String::new());
                    if asked.accepted {
                        page.friends_changed();
                        page.refresh();
                        Note::said(tf(
                            "Du bist mit {0} befreundet.",
                            &[&asked.name.unwrap_or_default()],
                        ))
                    } else {
                        page.refresh_asked(before);
                        Note::said(t(
                            "Anfrage gesendet. Ihr seid befreundet, sobald die Person dich auch anfragt.",
                        ))
                    }
                }
                Ok(state) => Note::failed(calm(&state, Context::Ask).unwrap_or_default()),
                Err(error) => Note::failed(error),
            };
            note.try_set(said);
            busy.try_set(false);
        });
    };
    view! {
        // The app checks the address itself and says so in its own words and language.
        <form class="friend-form keep-in-view" novalidate on:submit=move |event| {
            event.prevent_default();
            send();
        }>
            <label class="input">
                <span>{t("E-Mail-Adresse der Person")}</span>
                <input type="email" inputmode="email" autocomplete="off" autocapitalize="none"
                    spellcheck="false" maxlength="254" bind:value=email aria-describedby="ask-hint"
                    on:focus=move |event| crate::keep_in_view(event_target(&event)) />
            </label>
            <p class="notes" id="ask-hint">{t("Die Person sieht deinen Anzeigenamen und kann annehmen. Ob es zu der Adresse ein Konto gibt, verrät Locora nicht.")}</p>
            <button type="submit" class="primary" disabled=move || busy.get()>{t("Anfrage senden")}</button>
        </form>
        <Said note=note />
    }
}

/// The open invite links. Without access only revoking is offered (the server keeps that free, so
/// a link handed out earlier can always be stopped), and the part shows only while there are some;
/// with access the invite card carries its heading.
#[component]
fn OpenLinks(access: bool) -> impl IntoView {
    let page = expect_context::<Page>();
    let note = RwSignal::new(None);
    let links = move || {
        page.with(|list| match &list.invites {
            InviteList::Ready(links) => links.clone(),
            _ => Vec::new(),
        })
        .unwrap_or_default()
    };
    view! {
        <Show when=move || !access && !links().is_empty()>
            <h3>{t("Offene Einladungslinks")}</h3>
        </Show>
        <Show when=move || access && links().is_empty()>
            <p class="notes">{t("Keine offenen Einladungslinks.")}</p>
        </Show>
        <Show when=move || !links().is_empty()>
            <ul class="friend-rows">
                <For each=links key=|link| link.code.clone() children=move |link| view! {
                    <LinkRow link=link access=access note=note />
                } />
            </ul>
        </Show>
        <Said note=note />
    }
}

#[component]
fn LinkRow(link: InviteLink, access: bool, note: Notes) -> impl IntoView {
    let page = expect_context::<Page>();
    let busy = RwSignal::new(false);
    // Shown when copying failed, to select by hand.
    let by_hand = RwSignal::new(false);
    let field = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(field) = field.get() {
            let _ = field.focus();
            field.select();
        }
    });
    let code = StoredValue::new(link.code.clone());
    let copied_text = StoredValue::new(link.url.clone().unwrap_or_else(|| link.code.clone()));
    let copy = move |_| {
        let copied = crate::share::copy_text(&copied_text.get_value());
        spawn_local(async move {
            if copied.await {
                by_hand.try_set(false);
                note.try_set(Note::said(t("Kopiert.")));
            } else {
                note.try_set(Some(Note {
                    text: t("Kopieren nicht möglich. Bitte den Link hier markieren und kopieren.")
                        .into(),
                    alert: true,
                    focus: false,
                }));
                by_hand.try_set(true);
            }
        });
    };
    let revoke = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        let code = code.get_value();
        spawn_local(async move {
            let said = match call::<Social<()>>(
                CMD_CLOUD_INVITE_REVOKE,
                InviteArgs { code: code.clone() },
            )
            .await
            {
                Ok(Social::Ready(())) => {
                    if page
                        .new_link
                        .with_untracked(|link| link.as_ref().is_some_and(|link| link.code == code))
                    {
                        page.new_link.try_set(None);
                        page.link_note.try_set(None);
                    }
                    page.refresh();
                    Note::said(tf(
                        "Einladungslink {0} zurückgezogen. Er funktioniert nicht mehr.",
                        &[&code],
                    ))
                }
                Ok(state) => Note::failed(calm(&state, Context::Links).unwrap_or_default()),
                Err(error) => {
                    page.refresh();
                    Note::failed(error)
                }
            };
            note.try_set(said);
            busy.try_set(false);
        });
    };
    view! {
        <li class="friend-row">
            <div class="friend-line">
                <span class="friend-name"><span class="invite-code">{link.code.clone()}</span>" · "{tf("gilt bis {0}", &[&when(link.expires_at, false)])}</span>
                <div class="friend-actions">
                    {access.then(|| view! {
                        <button type="button" aria-label=tf("Einladungslink {0} kopieren", &[&code.get_value()]) on:click=copy>{t("Kopieren")}</button>
                    })}
                    <button type="button" disabled=move || busy.get()
                        aria-label=tf("Einladungslink {0} zurückziehen", &[&code.get_value()])
                        on:click=revoke>{t("Zurückziehen")}</button>
                </div>
            </div>
            <Show when=move || by_hand.get()>
                <input type="text" class="invite-url" readonly node_ref=field prop:value=copied_text.get_value()
                    aria-label=t("Einladungslink") />
            </Show>
        </li>
    }
}

/// Accepting an invite: the pasted link or code is checked first, and shows who invites. Only the
/// code checked is accepted, and any change of the text takes the answer away again.
#[component]
fn AcceptForm() -> impl IntoView {
    let page = expect_context::<Page>();
    let text = RwSignal::new(String::new());
    let preview = RwSignal::new(None::<InviteFrom>);
    let busy = RwSignal::new(false);
    let note = RwSignal::new(None);
    let field = NodeRef::<leptos::html::Input>::new();
    let first = NodeRef::<leptos::html::P>::new();
    Effect::new(move |_| {
        if let Some(first) = first.get() {
            let _ = first.focus();
        }
    });
    let check = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        note.set(None);
        preview.set(None);
        let code = text.get_untracked();
        spawn_local(async move {
            match call::<Social<InviteFrom>>(
                CMD_CLOUD_INVITE_PREVIEW,
                InviteArgs { code: code.clone() },
            )
            .await
            {
                // Only while the field still holds what was checked.
                Ok(Social::Ready(from)) => {
                    if text.try_get_untracked().as_deref() == Some(code.as_str()) {
                        preview.try_set(Some(from));
                    }
                }
                Ok(state) => {
                    note.try_set(Note::failed(
                        calm(&state, Context::Accept).unwrap_or_default(),
                    ));
                }
                Err(error) => {
                    note.try_set(Note::failed(error));
                }
            }
            busy.try_set(false);
        });
    };
    let accept = move || {
        let Some(from) = preview.get_untracked() else {
            return;
        };
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        spawn_local(async move {
            let said = match call::<Social<FriendEntry>>(
                CMD_CLOUD_INVITE_ACCEPT,
                InviteArgs {
                    code: from.code.clone(),
                },
            )
            .await
            {
                Ok(Social::Ready(friend)) => {
                    text.try_set(String::new());
                    preview.try_set(None);
                    page.friends_changed();
                    page.refresh();
                    Note::said(tf("Du bist mit {0} befreundet.", &[&friend.name]))
                }
                Ok(state) => Note::failed(calm(&state, Context::Accept).unwrap_or_default()),
                Err(error) => {
                    preview.try_set(None);
                    Note::failed(error)
                }
            };
            note.try_set(said);
            busy.try_set(false);
        });
    };
    view! {
        <form class="friend-form keep-in-view" on:submit=move |event| {
            event.prevent_default();
            check();
        }>
            <label class="input">
                <span>{t("Einladungslink oder Code")}</span>
                <input type="text" autocomplete="off" autocapitalize="characters" spellcheck="false"
                    maxlength="512" node_ref=field prop:value=move || text.get()
                    on:input=move |event| {
                        text.set(event_target_value(&event));
                        preview.set(None);
                    }
                    on:focus=move |event| crate::keep_in_view(event_target(&event)) />
            </label>
            <button type="submit" disabled=move || busy.get()>{t("Prüfen")}</button>
        </form>
        {move || preview.get().map(|from| {
            let name = StoredValue::new(from.name.clone());
            let reference = StoredValue::new(from.reference.clone());
            view! {
                <div class="invite-from" role="group" aria-labelledby="invite-from-line">
                    <p id="invite-from-line" tabindex="-1" node_ref=first>
                        <strong>{tf("{0} lädt dich ein.", &[&from.name])}</strong>
                    </p>
                    <p>{tf("Die Einladung gilt bis {0}.", &[&when(from.expires_at, false)])}</p>
                    <div class="friend-actions">
                        <button type="button" class="primary" disabled=move || busy.get() on:click=move |_| accept()>{t("Annehmen")}</button>
                        {from.reference.is_some().then(|| view! {
                            <button type="button" aria-label=tf("Namen „{0}“ melden", &[&name.get_value()])
                                on:click=move |event| {
                                    if let Some(reference) = reference.get_value() {
                                        page.report(&event, name.get_value(), reference, note);
                                    }
                                }>{t("Melden")}</button>
                        })}
                        <button type="button" on:click=move |_| {
                            preview.set(None);
                            text.set(String::new());
                            // The answer goes: the focus goes back to the field, for another code.
                            if let Some(field) = field.get_untracked() {
                                let _ = field.focus();
                            }
                        }>{t("Abbrechen")}</button>
                    </div>
                </div>
            }
        })}
        <Said note=note />
    }
}

/// What a ranking says besides its rows: why there are no points from the app, an empty list, a
/// server that takes no reports yet.
fn ranking_notes(list: &RankingList, friends: bool) -> Vec<&'static str> {
    let mut notes = Vec::new();
    if list.entries.is_empty() {
        notes.push(t("Noch keine Einträge."));
    } else if friends && list.entries.len() == 1 {
        notes.push(t(
            "Sobald du Freunde hast, vergleicht ihr hier eure Punkte.",
        ));
    }
    let own_zero = list.entries.iter().any(|row| row.own && row.points == 0);
    if !friends && list.without_you {
        notes.push(t("Du stehst noch nicht in dieser Rangliste."));
    }
    if own_zero || (!friends && list.without_you) {
        notes.push(t("Deine Journeys in der App zählen hier noch nicht mit. Ranglistenpunkte sammelt Locora Cloud nur aus Journeys, die es selbst nachrechnet."));
    }
    if !list.entries.is_empty() && list.entries.iter().all(|row| row.reference.is_none()) {
        notes.push(t("Dieser Server nimmt noch keine Meldungen von Namen an. Das geht, sobald er aktualisiert ist."));
    }
    notes
}

type Loaded = RwSignal<Option<Result<Social<RankingList>, String>>>;

/// „Rangliste“: the friends' ranking and everyone's, each read once when it is first shown.
#[component]
fn RankingCard() -> impl IntoView {
    let page = expect_context::<Page>();
    let friends_tab = RwSignal::new(true);
    let of_friends: Loaded = RwSignal::new(None);
    let of_everyone: Loaded = RwSignal::new(None);
    let note = RwSignal::new(None);
    let load = move |friends: bool| {
        let into = if friends { of_friends } else { of_everyone };
        spawn_local(async move {
            let result =
                call::<Social<RankingList>>(CMD_CLOUD_RANKING, RankingArgs { friends }).await;
            into.try_set(Some(result));
        });
    };
    Effect::new(move |_| {
        let friends = friends_tab.get();
        let into = if friends { of_friends } else { of_everyone };
        if into.with_untracked(Option::is_none) {
            load(friends);
        }
    });
    // A change of friends shows in their ranking (once it was read).
    Effect::new(move |previous: Option<u32>| {
        let now = page.friends_changed.get();
        if previous.is_some_and(|previous| previous != now)
            && of_friends.with_untracked(Option::is_some)
        {
            load(true);
        }
        now
    });
    let tab = move |friends: bool, label: &'static str| {
        view! {
            <button type="button" aria-pressed=move || if friends_tab.get() == friends { "true" } else { "false" }
                on:click=move |_| friends_tab.set(friends)>{label}</button>
        }
    };
    // A retry shows the ranking loading again; the focus waits on the heading (the button goes).
    let heading = NodeRef::<leptos::html::H2>::new();
    view! {
        <section class="card ranking-card" aria-labelledby="ranking-title">
            <div class="info-head">
                <h2 id="ranking-title" tabindex="-1" node_ref=heading>{t("Rangliste")}</h2>
                <Info text="Ranglistenpunkte sammelt Locora Cloud aus Journeys, die es selbst nachrechnet. Die XP auf deinen Geräten zählen hier nicht mit." />
            </div>
            <div class="ranking-tabs" role="group" aria-label=t("Rangliste")>
                {tab(true, t("Freunde"))}
                {tab(false, t("Alle"))}
            </div>
            {move || {
                let friends = friends_tab.get();
                let loaded = if friends { of_friends } else { of_everyone };
                match loaded.get() {
                    None => view! { <p class="progress" role="status">{t("Rangliste wird geladen …")}</p> }.into_any(),
                    Some(Err(error)) => view! {
                        <p class="notes" role="alert">{error}</p>
                        <button type="button" on:click=move |_| {
                            if let Some(heading) = heading.get_untracked() {
                                let _ = heading.focus();
                            }
                            loaded.set(None);
                            load(friends);
                        }>{t("Erneut versuchen")}</button>
                    }.into_any(),
                    Some(Ok(Social::Ready(list))) => {
                        let notes = ranking_notes(&list, friends);
                        view! {
                            <ol class="ranking">
                                {list.entries.into_iter().map(|row| view! { <RankingEntry row=row note=note /> }).collect_view()}
                            </ol>
                            {notes.into_iter().map(|text| view! { <p class="notes">{text}</p> }).collect_view()}
                        }.into_any()
                    }
                    Some(Ok(state)) => view! {
                        <p class="notes">{calm(&state, Context::Ranking).unwrap_or_default()}</p>
                    }.into_any(),
                }
            }}
            <Said note=note />
        </section>
    }
}

/// The space before „(du)“; none before full-width brackets, which bring their own.
fn you_gap() -> &'static str {
    if t("(du)").starts_with('（') {
        ""
    } else {
        " "
    }
}

#[component]
fn RankingEntry(row: RankingRow, note: Notes) -> impl IntoView {
    let page = expect_context::<Page>();
    let name = StoredValue::new(row.name.clone());
    let reference = StoredValue::new(row.reference.clone());
    view! {
        <li class="ranking-row">
            <span class="ranking-place">{format!("{}.", row.place)}</span>
            <span class="ranking-who">
                <bdi>{row.name.clone()}</bdi>
                {row.own.then(|| view! { {you_gap()}<span class="ranking-you">{t("(du)")}</span> })}
            </span>
            <span class="ranking-points">{points_text(row.points)}</span>
            // The own entry is not offered for reporting. It is marked only where exactly one
            // entry carries the account's name, so no one else's button is hidden by a copy.
            {(row.reference.is_some() && !row.own).then(|| view! {
                <button type="button" class="ranking-report" aria-label=tf("Namen „{0}“ melden", &[&name.get_value()])
                    on:click=move |event| {
                        if let Some(reference) = reference.get_value() {
                            page.report(&event, name.get_value(), reference, note);
                        }
                    }>{t("Melden")}</button>
            })}
        </li>
    }
}

/// Reporting a name, with an optional reason.
#[component]
fn ReportDialog(target: ReportTarget) -> impl IntoView {
    let page = expect_context::<Page>();
    let reason = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    let problem = RwSignal::new(None::<Note>);
    let close = Callback::new(move |()| page.report.set(None));
    let reference = StoredValue::new(target.reference.clone());
    let note = target.note;
    let send = move || {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        problem.set(None);
        let text = reason.get_untracked();
        let args = ReportArgs {
            reference: reference.get_value(),
            reason: (!text.trim().is_empty()).then_some(text),
        };
        spawn_local(async move {
            match call::<Social<Reported>>(CMD_CLOUD_REPORT, args).await {
                Ok(Social::Ready(reported)) => {
                    let mut said =
                        t("Gemeldet. Danke – gemeldete Namen werden geprüft.").to_owned();
                    if !reported.counts_now {
                        said.push(' ');
                        said.push_str(t(
                            "Deine Meldung zählt, sobald deine E-Mail-Adresse bestätigt ist.",
                        ));
                    }
                    // The focus goes back to the button that opened the dialog; the line says it.
                    note.try_set(Some(Note {
                        text: said,
                        alert: false,
                        focus: false,
                    }));
                    page.report.try_set(None);
                }
                Ok(state) => {
                    problem.try_set(Note::failed(
                        calm(&state, Context::Report).unwrap_or_default(),
                    ));
                }
                Err(error) => {
                    problem.try_set(Note::failed(error));
                }
            }
            busy.try_set(false);
        });
    };
    view! {
        <Modal labelled="report-title" described="report-what" class="report-dialog"
            dirty=Signal::derive(move || !reason.get().trim().is_empty())
            opener=page.report_opener.get_value() on_close=close>
            <h2 id="report-title" tabindex="-1">{t("Namen melden")}</h2>
            <p id="report-what">{tf("Du meldest „{0}“ als beleidigend oder unpassend. Nach mehreren Meldungen sehen andere statt des Namens „Nutzer-…“, bis er geprüft ist.", &[&target.name])}</p>
            <form class="keep-in-view" on:submit=move |event| {
                event.prevent_default();
                send();
            }>
                <label class="input">
                    <span>{t("Grund (freiwillig, höchstens 300 Zeichen)")}</span>
                    <textarea rows="3" maxlength="300" prop:value=move || reason.get()
                        on:input=move |event| reason.set(event_target_value(&event))
                        on:focus=move |event| crate::keep_in_view(event_target(&event))></textarea>
                </label>
                <div class="friend-actions">
                    <button type="submit" class="primary" disabled=move || busy.get()>{t("Melden")}</button>
                    <button type="button" on:click=move |_| close.run(())>{t("Abbrechen")}</button>
                </div>
            </form>
            <Said note=problem />
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_state_has_calm_words_in_every_context() {
        let contexts = [
            Context::Page,
            Context::Ask,
            Context::Remove,
            Context::Ranking,
            Context::Report,
            Context::Links,
            Context::Accept,
        ];
        let states: [Social<()>; 4] = [
            Social::SignedOut,
            Social::SignInAgain,
            Social::NotForAccount,
            Social::ServerTooOld,
        ];
        for lang in crate::i18n::Lang::ALL {
            crate::i18n::set_current(lang);
            for state in &states {
                for context in contexts {
                    let text = calm(state, context).unwrap_or_default();
                    assert!(!text.is_empty(), "{lang:?} {state:?} {context:?}");
                    for needle in ["402", "404", "405", "Abo", "Nicht gefunden", "subscription"] {
                        assert!(
                            !text.contains(needle),
                            "{lang:?} {state:?} {context:?}: {text}"
                        );
                    }
                    assert!(
                        crate::store_rules::selling(text).is_empty(),
                        "{lang:?}: {text}"
                    );
                }
            }
            assert_eq!(calm(&Social::Ready(()), Context::Page), None);
        }
        crate::i18n::set_current(crate::i18n::Lang::En);
        assert_eq!(points_text(1), "1 ranking point");
        assert_eq!(points_text(0), "0 ranking points");
        crate::i18n::set_current(crate::i18n::Lang::De);
        assert_eq!(points_text(80), "80 Ranglistenpunkte");
    }

    #[test]
    fn a_ranking_explains_itself_where_the_app_earns_nothing() {
        crate::i18n::set_current(crate::i18n::Lang::De);
        let row = |own: bool, points: u64, reference: bool| RankingRow {
            place: 1,
            name: "Ada".into(),
            points,
            reference: reference.then(|| "n1".into()),
            own,
        };
        let alone = RankingList {
            entries: vec![row(true, 0, true)],
            without_you: false,
        };
        let notes = ranking_notes(&alone, true);
        assert!(
            notes
                .iter()
                .any(|note| note.starts_with("Sobald du Freunde hast"))
        );
        assert!(
            notes
                .iter()
                .any(|note| note.starts_with("Deine Journeys in der App"))
        );
        let everyone = RankingList {
            entries: vec![row(false, 9, false)],
            without_you: true,
        };
        let notes = ranking_notes(&everyone, false);
        assert!(notes.contains(&"Du stehst noch nicht in dieser Rangliste."));
        assert!(
            notes
                .iter()
                .any(|note| note.starts_with("Dieser Server nimmt noch keine Meldungen"))
        );
        let scored = RankingList {
            entries: vec![row(true, 5, true), row(false, 3, true)],
            without_you: false,
        };
        assert!(ranking_notes(&scored, true).is_empty());
        let empty = RankingList {
            entries: Vec::new(),
            without_you: true,
        };
        assert_eq!(ranking_notes(&empty, true)[0], "Noch keine Einträge.");
    }

    #[test]
    fn the_own_mark_keeps_the_spacing_of_its_language() {
        for (lang, gap) in [
            (crate::i18n::Lang::De, " "),
            (crate::i18n::Lang::En, " "),
            (crate::i18n::Lang::Fr, " "),
            (crate::i18n::Lang::Ja, ""),
            (crate::i18n::Lang::Zh, ""),
        ] {
            crate::i18n::set_current(lang);
            assert_eq!(you_gap(), gap, "{lang:?}: {}", t("(du)"));
        }
        crate::i18n::set_current(crate::i18n::Lang::De);
    }

    #[test]
    fn the_social_views_render_names_as_text() {
        let needles = [concat!("inner", "_html"), concat!("inner", "HTML")];
        for (file, source) in [
            ("friends.rs", include_str!("friends.rs")),
            ("modal.rs", include_str!("modal.rs")),
            ("share.rs", include_str!("share.rs")),
            ("share_card.rs", include_str!("share_card.rs")),
        ] {
            for needle in needles {
                assert!(!source.contains(needle), "{file} sets markup: {needle}");
            }
        }
    }
}
