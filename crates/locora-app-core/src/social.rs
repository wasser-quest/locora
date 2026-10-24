//! Friends, invite links, rankings and name reports as the UI sees them, the same for the desktop
//! and the browser. Each command is a flow (see `locora_runtime::cloud_http`): the desktop runs it
//! with `cloud_http::run_answering`, the browser with `fetch`; in both a call without a reply comes
//! back as status 0. Its replies are read here, so that an expired session (401), an account
//! without access (402) and an older server (404/405) come out as `Social` states that the UI
//! words itself, never as a server's message. Names chosen by other people are made safe to show
//! (`locora_runtime::shown`), and an invite link from the server is kept only when it is the
//! account site's invite page for its code.
//!
//! The arguments are checked here, before anything is sent, so both hosts refuse the same.

use anyhow::{Result, anyhow, bail};
use locora_app_api::{
    FriendAsked, FriendEntry, FriendsList, InviteFrom, InviteLink, InviteList, RankingList,
    RankingRow, Reported, SentRequest, Social,
};
use locora_cloud_api::api::{
    Account, Friend, FriendInvite, InvitePreview, Ranking, parse_invite_code,
};
use locora_runtime::cloud::{self, CloudState, FriendKind};
use locora_runtime::cloud_http::{self, Call, Flow, Reply, Step};
use locora_runtime::shown;

/// Characters of an e-mail address (the longest there is).
const EMAIL_MAX: usize = 254;
/// Characters of a pasted invite link or code: a shared link may carry a long query.
const PASTED_MAX: usize = 512;
/// Characters of an id, a stored invite code or a reference of a name.
const ID_MAX: usize = 128;
/// Characters of the reason of a report (the server's limit).
pub const REASON_MAX: usize = 300;

/// A pasted text without an invite code in it.
const NO_CODE: &str =
    "Das ist kein Einladungscode (wie ABCD-EFGH-JKLM-NPQR) und kein Einladungslink.";
/// The limit of open requests (409). The server's own message names a number it can change.
const REQUESTS_LIMIT: &str = "Du hast die Höchstzahl offener Anfragen erreicht. Eine neue geht, sobald eine angenommen, zurückgezogen oder 30 Tage alt ist.";
/// The limit of open invite links (409), in the app's own words for the same reason.
const LINKS_LIMIT: &str = "Du hast die Höchstzahl offener Einladungslinks erreicht. Ein neuer geht, sobald einer angenommen, zurückgezogen oder abgelaufen ist.";

type Read<T> = Box<dyn FnOnce(Reply) -> Result<T> + Send>;

/// One call whose reply `read` turns into the result; unlike `cloud_http::Exchange` the reader
/// may keep what the call was about (the own display name, the code).
pub struct One<T> {
    call: Option<Call>,
    read: Option<Read<T>>,
}

impl<T> One<T> {
    fn new(call: Call, read: impl FnOnce(Reply) -> Result<T> + Send + 'static) -> Self {
        Self {
            call: Some(call),
            read: Some(Box::new(read)),
        }
    }
}

impl<T> Flow for One<T> {
    type Output = T;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<T>> {
        match reply {
            None => match self.call.take() {
                Some(call) => Ok(Step::Call(call)),
                None => bail!("the request was already sent"),
            },
            Some(reply) => match self.read.take() {
                Some(read) => Ok(Step::Done(read(reply)?)),
                None => bail!("the reply was already read"),
            },
        }
    }
}

/// The state for a reply that has none of the result: 401, 402, or an older server. `older` is
/// `cloud_http::unsupported` where a newer server never answers 404 or 405, and
/// `cloud_http::no_route` where it answers 404 itself for something it does not find.
fn unavailable<T>(reply: &Reply, older: fn(&Reply) -> bool) -> Option<Social<T>> {
    match reply.status {
        401 => Some(Social::SignInAgain),
        402 => Some(Social::NotForAccount),
        _ if older(reply) => Some(Social::ServerTooOld),
        _ => None,
    }
}

/// An id, a stored code or a reference the UI hands back, as it is sent.
fn checked(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > ID_MAX {
        bail!("Ungültige Anfrage");
    }
    Ok(text.to_owned())
}

/// The invite code in a pasted link or code, read before anything is sent. A whole message as the
/// app shares a code without a link („… (gilt bis …): ABCD-…“, in any language) is read too: its
/// code is the last word that reads as one.
fn pasted_code(text: &str) -> Result<String> {
    if text.chars().count() > PASTED_MAX {
        bail!(NO_CODE);
    }
    parse_invite_code(text)
        .or_else(|| {
            text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .rev()
                .find_map(parse_invite_code)
        })
        .ok_or_else(|| anyhow!(NO_CODE))
}

/// The friends page: the friend list and the open invite links, asked side by side.
pub struct FriendsPage {
    calls: Option<Vec<Call>>,
}

pub fn friends(state: &CloudState) -> Result<FriendsPage> {
    Ok(FriendsPage {
        calls: Some(vec![
            cloud::friends_call(state)?,
            cloud::invites_call(state)?,
        ]),
    })
}

impl Flow for FriendsPage {
    type Output = Social<FriendsList>;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Social<FriendsList>>> {
        match (reply, self.calls.take()) {
            (None, Some(calls)) => Ok(Step::Calls(calls)),
            _ => bail!("the friends page asks for its two calls side by side"),
        }
    }

    fn step_many(&mut self, replies: Vec<Reply>) -> Result<Step<Social<FriendsList>>> {
        let [list, links]: [Reply; 2] = replies
            .try_into()
            .map_err(|_| anyhow!("the friends page needs both replies"))?;
        Ok(Step::Done(friends_page(list, links)?))
    }
}

/// The page from the friend list and the invite links. The links need no subscription on the
/// server, so an account without access still sees its open links, to revoke them. A failing
/// link list leaves the rest of the page as it is.
fn friends_page(list: Reply, links: Reply) -> Result<Social<FriendsList>> {
    let invites = invite_list(links);
    let empty = |access: bool, invites: InviteList| FriendsList {
        access,
        incoming: Vec::new(),
        friends: Vec::new(),
        sent: Vec::new(),
        open: 0,
        invites,
    };
    if list.status == 402 {
        return Ok(match invites {
            InviteList::Ready(links) => Social::Ready(empty(false, InviteList::Ready(links))),
            _ => Social::NotForAccount,
        });
    }
    if let Some(state) = unavailable(&list, cloud_http::unsupported) {
        return Ok(state);
    }
    let mut page = empty(true, invites);
    for friend in cloud::friends_reply(list)? {
        match cloud::friend_kind(&friend) {
            FriendKind::Incoming => page.incoming.push(entry(&friend)),
            FriendKind::Friend => page.friends.push(entry(&friend)),
            FriendKind::Outgoing => page.sent.push(SentRequest {
                id: friend.id,
                since: friend.since,
            }),
            FriendKind::Open => page.open += 1,
        }
    }
    Ok(Social::Ready(page))
}

/// A friend or the sender of a request, its name made safe to show.
fn entry(friend: &Friend) -> FriendEntry {
    FriendEntry {
        id: friend.id.clone(),
        name: shown::name(&friend.display_name),
    }
}

/// The open invite links: an older server has none; another failure is shown with them only.
fn invite_list(reply: Reply) -> InviteList {
    if cloud_http::unsupported(&reply) {
        return InviteList::ServerTooOld;
    }
    match cloud_http::json::<Vec<FriendInvite>>(reply) {
        Ok(invites) => InviteList::Ready(invites.into_iter().filter_map(link).collect()),
        Err(error) => InviteList::Failed(error.to_string()),
    }
}

/// An invite link from the server; one whose code does not read as a code is left out (it would
/// not work anyway).
fn link(invite: FriendInvite) -> Option<InviteLink> {
    let code = parse_invite_code(&invite.code)?;
    Some(InviteLink {
        url: invite_page(&invite.url, &code),
        code,
        expires_at: invite.expires_at,
    })
}

/// The link when it is an invite page for `code`: HTTPS, no user, no query, the path
/// `/einladung` and the code as its fragment. A server that sends anything else (another page, a
/// shop) gets its link left out; the code alone is shown, copied and shared then.
pub fn invite_page(url: &str, code: &str) -> Option<String> {
    if url.len() > 2048 {
        return None;
    }
    let parsed = url::Url::parse(url).ok()?;
    let fine = parsed.scheme() == "https"
        && parsed.host_str().is_some()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.path() == "/einladung"
        && parsed
            .fragment()
            .is_some_and(|fragment| parse_invite_code(fragment).as_deref() == Some(code));
    fine.then(|| parsed.to_string())
}

/// Asks the account behind `email` to be friends; asking someone who asked first accepts. The
/// address is checked here and never comes back: a request that waits names nobody.
pub fn ask(state: &CloudState, email: &str) -> Result<One<Social<FriendAsked>>> {
    let email = email.trim();
    if email.is_empty() || email.chars().count() > EMAIL_MAX || !email.contains('@') {
        bail!("Eine gültige E-Mail-Adresse eingeben");
    }
    Ok(One::new(
        cloud::request_friend_call(state, email)?,
        |reply| {
            if let Some(state) = unavailable(&reply, cloud_http::unsupported) {
                return Ok(state);
            }
            if reply.status == 409 {
                bail!(REQUESTS_LIMIT);
            }
            let friend = cloud::request_friend_reply(reply)?;
            Ok(Social::Ready(FriendAsked {
                accepted: friend.accepted,
                // An older server names the account asked even while it waits: never shown.
                name: friend.accepted.then(|| shown::name(&friend.display_name)),
            }))
        },
    ))
}

/// Ends a friendship, withdraws a request or declines one (`id` from the list).
pub fn remove(state: &CloudState, id: &str) -> Result<One<Social<()>>> {
    let id = checked(id)?;
    Ok(One::new(cloud::remove_friend_call(state, &id)?, |reply| {
        if let Some(state) = unavailable(&reply, cloud_http::no_route) {
            return Ok(state);
        }
        cloud_http::expect_success(reply)?;
        Ok(Social::Ready(()))
    }))
}

/// The ranking of the friends (with this account) or of everyone.
pub fn ranking(state: &CloudState, friends: bool) -> Result<One<Social<RankingList>>> {
    let own = state.account.display_name.clone();
    Ok(One::new(
        cloud::ranking_call(state, friends)?,
        move |reply| {
            if let Some(state) = unavailable(&reply, cloud_http::unsupported) {
                return Ok(state);
            }
            let ranking = cloud::ranking_reply(reply)?;
            Ok(Social::Ready(ranking_list(ranking, &own, friends)))
        },
    ))
}

/// The rows with their places (equal points share one) and names made safe. The server sends no
/// mark for this account's own entry, and names are not unique: the own entry is marked only in
/// the friends ranking, which always holds this account, and only when no friend has its name.
fn ranking_list(ranking: Ranking, own: &str, friends: bool) -> RankingList {
    let named = ranking
        .entries
        .iter()
        .filter(|entry| entry.display_name == own)
        .count();
    let mut place = 0;
    let mut previous = None;
    let entries = ranking
        .entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            if previous != Some(entry.xp) {
                place = u32::try_from(index + 1).unwrap_or(u32::MAX);
                previous = Some(entry.xp);
            }
            RankingRow {
                place,
                own: friends && named == 1 && entry.display_name == own,
                name: shown::name(&entry.display_name),
                points: entry.xp,
                reference: entry.reference,
            }
        })
        .collect();
    RankingList {
        entries,
        without_you: named == 0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReportStage {
    Report,
    Account,
}

/// Reports a name. A report counts once this account's address is confirmed: when the stored
/// account says it is not, the account is asked again once, only to word the answer. Nothing is
/// saved from it (a running sync may be writing the account's file).
pub struct Report {
    state: CloudState,
    call: Option<Call>,
    stage: ReportStage,
}

pub fn report(state: &CloudState, reference: &str, reason: Option<&str>) -> Result<Report> {
    let reference = checked(reference)?;
    let reason = reason.map(str::trim).filter(|reason| !reason.is_empty());
    if reason.is_some_and(|reason| reason.chars().count() > REASON_MAX) {
        bail!("Der Grund darf höchstens 300 Zeichen haben");
    }
    Ok(Report {
        call: Some(cloud::report_name_call(state, &reference, reason)?),
        state: state.clone(),
        stage: ReportStage::Report,
    })
}

impl Flow for Report {
    type Output = Social<Reported>;

    fn step(&mut self, reply: Option<Reply>) -> Result<Step<Social<Reported>>> {
        let Some(reply) = reply else {
            return match self.call.take() {
                Some(call) => Ok(Step::Call(call)),
                None => bail!("the report was already sent"),
            };
        };
        match self.stage {
            ReportStage::Report => {
                if let Some(state) = unavailable(&reply, cloud_http::unsupported) {
                    return Ok(Step::Done(state));
                }
                cloud_http::expect_success(reply)?;
                if self.state.account.email_verified != Some(false) {
                    return Ok(Step::Done(Social::Ready(Reported { counts_now: true })));
                }
                self.stage = ReportStage::Account;
                Ok(Step::Call(cloud::me_call(&self.state)?))
            }
            // The report is done; an account that cannot be read counts as still unconfirmed.
            ReportStage::Account => {
                let counts_now = cloud_http::json::<Account>(reply)
                    .is_ok_and(|account| account.email_verified != Some(false));
                Ok(Step::Done(Social::Ready(Reported { counts_now })))
            }
        }
    }
}

/// A new invite link.
pub fn create_invite(state: &CloudState) -> Result<One<Social<InviteLink>>> {
    Ok(One::new(cloud::create_invite_call(state)?, |reply| {
        if let Some(state) = unavailable(&reply, cloud_http::unsupported) {
            return Ok(state);
        }
        if reply.status == 409 {
            bail!(LINKS_LIMIT);
        }
        let invite = cloud_http::json::<FriendInvite>(reply)?;
        link(invite)
            .map(Social::Ready)
            .ok_or_else(|| anyhow!("Antwort von Locora Cloud ist ungültig"))
    }))
}

/// Revokes an open invite link (its code as the list has it).
pub fn revoke_invite(state: &CloudState, code: &str) -> Result<One<Social<()>>> {
    let code = parse_invite_code(&checked(code)?).ok_or_else(|| anyhow!(NO_CODE))?;
    Ok(One::new(
        cloud::revoke_invite_call(state, &code)?,
        |reply| {
            if let Some(state) = unavailable(&reply, cloud_http::no_route) {
                return Ok(state);
            }
            cloud_http::expect_success(reply)?;
            Ok(Social::Ready(()))
        },
    ))
}

/// Who invites with a pasted link or code, asked without the session (the server answers it for
/// anyone with the code).
pub fn preview_invite(state: &CloudState, text: &str) -> Result<One<Social<InviteFrom>>> {
    let code = pasted_code(text)?;
    Ok(One::new(
        cloud::invite_preview_call(&state.server, &code)?,
        move |reply| {
            if let Some(state) = unavailable(&reply, cloud_http::no_route) {
                return Ok(state);
            }
            let preview = cloud_http::json::<InvitePreview>(reply)?;
            Ok(Social::Ready(InviteFrom {
                code,
                name: shown::name(&preview.display_name),
                expires_at: preview.expires_at,
                reference: preview.reference,
            }))
        },
    ))
}

/// Accepts the invite with `code` (the one previewed); both are friends then.
pub fn accept_invite(state: &CloudState, code: &str) -> Result<One<Social<FriendEntry>>> {
    let code = pasted_code(code)?;
    Ok(One::new(
        cloud::accept_invite_call(state, &code)?,
        |reply| {
            if let Some(state) = unavailable(&reply, cloud_http::no_route) {
                return Ok(state);
            }
            let friend = cloud_http::json::<Friend>(reply)?;
            Ok(Social::Ready(entry(&friend)))
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use locora_cloud_api::api::SubscriptionState;
    use locora_runtime::cloud_http::{Body, Method};

    /// The server's answer without a subscription; its words must never reach the UI.
    const PAY: &str = r#"{"error":"Für diese Funktion ist ein aktives Abo nötig"}"#;

    fn state(verified: Option<bool>) -> CloudState {
        CloudState {
            server: "https://api.example.com".into(),
            token: "t".into(),
            account: Account {
                id: "me".into(),
                email: "lea@example.org".into(),
                display_name: "Lea".into(),
                subscription: SubscriptionState::Active,
                plan: None,
                period_end: None,
                email_verified: verified,
            },
            sync_generation: 0,
            entitlement: None,
            last_sync_at: None,
            auto_sync: false,
            key_proof_sent_at: None,
        }
    }

    fn reply(status: u16, body: &str) -> Reply {
        Reply {
            status,
            body: body.as_bytes().to_vec(),
        }
    }

    /// The one call a flow asks for first.
    fn first<F: Flow>(flow: &mut F) -> Call {
        match flow.step(None) {
            Ok(Step::Call(call)) => call,
            _ => panic!("the flow asks for one call first"),
        }
    }

    /// Runs a one-call flow with `answer` and returns its result.
    fn answered<T>(mut flow: One<T>, answer: Reply) -> Result<T> {
        first(&mut flow);
        match flow.step(Some(answer))? {
            Step::Done(output) => Ok(output),
            _ => panic!("one call, then done"),
        }
    }

    fn no_subscription_words<T: serde::Serialize>(output: &T) {
        let text = serde_json::to_string(output).unwrap();
        assert!(!text.contains("Abo"), "{text}");
    }

    fn page(list: Reply, links: Reply) -> Result<Social<FriendsList>> {
        let mut flow = friends(&state(Some(true))).unwrap();
        match flow.step_many(vec![list, links])? {
            Step::Done(page) => Ok(page),
            _ => panic!("done after both replies"),
        }
    }

    const LINKS: &str = r#"[{"code":"ABCD-EFGH-JKLM-NPQR","url":"https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR","expires_at":100}]"#;

    #[test]
    fn the_friends_page_asks_for_the_list_and_the_links_side_by_side() {
        let mut flow = friends(&state(Some(true))).unwrap();
        let Ok(Step::Calls(calls)) = flow.step(None) else {
            panic!("two calls side by side")
        };
        let urls: Vec<&str> = calls.iter().map(|call| call.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://api.example.com/v1/friends",
                "https://api.example.com/v1/friends/invites"
            ]
        );
        assert!(calls.iter().all(|call| call.method == Method::Get));
        let list = r#"[
            {"display_name":"Ada\u202eevil","accepted":true,"id":"f1"},
            {"display_name":"Ben","accepted":false,"id":"f2"},
            {"display_name":"","accepted":false,"id":"f3","since":20},
            {"display_name":"","accepted":false,"id":"f4","since":10},
            {"display_name":"Old","accepted":true},
            {"display_name":"Cleo","accepted":false}
        ]"#;
        let Social::Ready(page) = page(reply(200, list), reply(200, LINKS)).unwrap() else {
            panic!("ready")
        };
        assert!(page.access);
        assert_eq!(
            page.friends,
            [
                FriendEntry {
                    id: Some("f1".into()),
                    name: "Ada\\u{202e}evil".into()
                },
                FriendEntry {
                    id: None,
                    name: "Old".into()
                }
            ]
        );
        assert_eq!(
            page.incoming,
            [FriendEntry {
                id: Some("f2".into()),
                name: "Ben".into()
            }]
        );
        let sent: Vec<Option<u64>> = page.sent.iter().map(|sent| sent.since).collect();
        assert_eq!(sent, [Some(20), Some(10)], "the server's order is kept");
        assert_eq!(page.open, 1);
        assert_eq!(
            page.invites,
            InviteList::Ready(vec![InviteLink {
                code: "ABCD-EFGH-JKLM-NPQR".into(),
                url: Some("https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR".into()),
                expires_at: 100,
            }])
        );
    }

    #[test]
    fn the_friends_page_reads_states_not_messages() {
        assert_eq!(
            page(reply(401, "{}"), reply(401, "{}")).unwrap(),
            Social::SignInAgain
        );
        let none = page(reply(402, PAY), reply(200, "[]")).unwrap();
        no_subscription_words(&none);
        let Social::Ready(none) = none else {
            panic!("the (empty) links are shown")
        };
        assert!(!none.access && none.invites == InviteList::Ready(Vec::new()));
        // Without access the open links stay, to be revoked; the lists are empty.
        let links = page(reply(402, PAY), reply(200, LINKS)).unwrap();
        no_subscription_words(&links);
        let Social::Ready(links) = links else {
            panic!("the links are shown")
        };
        assert!(!links.access && links.friends.is_empty() && links.incoming.is_empty());
        assert!(matches!(&links.invites, InviteList::Ready(list) if list.len() == 1));
        assert_eq!(
            page(reply(402, PAY), reply(503, "{}")).unwrap(),
            Social::NotForAccount
        );
        assert_eq!(
            page(reply(402, PAY), reply(405, "")).unwrap(),
            Social::NotForAccount
        );
        for older in [reply(404, r#"{"error":"Nicht gefunden"}"#), reply(405, "")] {
            assert_eq!(page(older, reply(405, "")).unwrap(), Social::ServerTooOld);
        }
        let Social::Ready(older) = page(reply(200, "[]"), reply(405, "")).unwrap() else {
            panic!("ready")
        };
        assert_eq!(older.invites, InviteList::ServerTooOld);
        // A busy server for the links leaves the rest of the page.
        let Social::Ready(busy) = page(reply(200, "[]"), reply(503, "{}")).unwrap() else {
            panic!("ready")
        };
        assert!(
            matches!(&busy.invites, InviteList::Failed(text) if text.starts_with("Locora Cloud ist gerade ausgelastet"))
        );
        assert!(page(reply(503, "{}"), reply(200, "[]")).is_err());
        // A link list that got no reply at all (status 0, on the desktop as in the browser) leaves
        // the rest of the page too; a list without a reply is an error.
        let Social::Ready(lost) = page(reply(200, "[]"), reply(0, "")).unwrap() else {
            panic!("ready")
        };
        assert_eq!(
            lost.invites,
            InviteList::Failed("Locora Cloud nicht erreichbar".into())
        );
        assert_eq!(
            page(reply(0, ""), reply(200, "[]"))
                .unwrap_err()
                .to_string(),
            "Locora Cloud nicht erreichbar"
        );
    }

    #[test]
    fn an_invite_link_is_kept_only_as_the_invite_page_of_its_code() {
        let code = "ABCD-EFGH-JKLM-NPQR";
        assert_eq!(
            invite_page(
                "https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR",
                code
            )
            .as_deref(),
            Some("https://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR")
        );
        // A self-hosted server has its own site.
        assert!(
            invite_page(
                "https://cloud.example.org/einladung#abcd-efgh-jklm-npqr",
                code
            )
            .is_some()
        );
        for foreign in [
            "https://shop.example.org/checkout#ABCD-EFGH-JKLM-NPQR",
            "http://web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR",
            "https://web.locora.cloud/einladung?plan=year#ABCD-EFGH-JKLM-NPQR",
            "https://user@web.locora.cloud/einladung#ABCD-EFGH-JKLM-NPQR",
            "https://web.locora.cloud/einladung/more#ABCD-EFGH-JKLM-NPQR",
            "https://web.locora.cloud/einladung#ZZZZ-EFGH-JKLM-NPQR",
            "https://web.locora.cloud/einladung",
            "javascript:alert(1)#ABCD-EFGH-JKLM-NPQR",
            "not a link",
        ] {
            assert_eq!(invite_page(foreign, code), None, "{foreign}");
        }
        let foreign = r#"[{"code":"ABCD-EFGH-JKLM-NPQR","url":"https://shop.example.org/buy","expires_at":1},{"code":"<b>","url":"x","expires_at":1}]"#;
        let Social::Ready(list) = page(reply(200, "[]"), reply(200, foreign)).unwrap() else {
            panic!("ready")
        };
        assert_eq!(
            list.invites,
            InviteList::Ready(vec![InviteLink {
                code: code.into(),
                url: None,
                expires_at: 1
            }])
        );
    }

    #[test]
    fn asking_back_accepts_and_a_waiting_request_names_nobody() {
        let me = state(Some(true));
        for bad in [
            "",
            "   ",
            "no-at-sign",
            &format!("{}@x.org", "a".repeat(260)),
        ] {
            assert!(ask(&me, bad).is_err(), "{bad}");
        }
        let mut flow = ask(&me, " ben@example.org ").unwrap();
        let call = first(&mut flow);
        assert_eq!(
            call.body,
            Some(Body::Json(br#"{"email":"ben@example.org"}"#.to_vec()))
        );
        let waiting = answered(
            ask(&me, "ben@example.org").unwrap(),
            reply(201, r#"{"display_name":"","accepted":false}"#),
        )
        .unwrap();
        assert_eq!(
            waiting,
            Social::Ready(FriendAsked {
                accepted: false,
                name: None
            })
        );
        // An older server names the account asked while it waits: not shown.
        let older = answered(
            ask(&me, "ben@example.org").unwrap(),
            reply(201, r#"{"display_name":"Ben","accepted":false}"#),
        )
        .unwrap();
        assert_eq!(
            older,
            Social::Ready(FriendAsked {
                accepted: false,
                name: None
            })
        );
        let accepted = answered(
            ask(&me, "ben@example.org").unwrap(),
            reply(
                200,
                r#"{"display_name":"Ben\u0007","accepted":true,"id":"f2"}"#,
            ),
        )
        .unwrap();
        assert_eq!(
            accepted,
            Social::Ready(FriendAsked {
                accepted: true,
                name: Some("Ben\\u{7}".into())
            })
        );
        let full = answered(
            ask(&me, "ben@example.org").unwrap(),
            reply(
                409,
                r#"{"error":"Du hast schon 50 offene Freundschaftsanfragen"}"#,
            ),
        )
        .unwrap_err();
        assert!(
            full.to_string()
                .starts_with("Du hast die Höchstzahl offener Anfragen")
        );
        let none = answered(ask(&me, "ben@example.org").unwrap(), reply(402, PAY)).unwrap();
        assert_eq!(none, Social::NotForAccount);
        no_subscription_words(&none);
        let refused = answered(
            ask(&me, "lea@example.org").unwrap(),
            reply(400, r#"{"error":"Du kannst dich nicht selbst hinzufügen"}"#),
        )
        .unwrap_err();
        assert_eq!(
            refused.to_string(),
            "Du kannst dich nicht selbst hinzufügen"
        );
    }

    #[test]
    fn removing_tells_an_older_server_from_a_gone_entry() {
        let me = state(Some(true));
        assert!(remove(&me, "").is_err());
        assert!(remove(&me, &"x".repeat(129)).is_err());
        let mut flow = remove(&me, "fr-1").unwrap();
        let call = first(&mut flow);
        assert_eq!(call.method, Method::Delete);
        assert!(call.url.ends_with("/v1/friends/fr%2D1"), "{}", call.url);
        assert_eq!(
            answered(remove(&me, "f1").unwrap(), reply(204, "")).unwrap(),
            Social::Ready(())
        );
        for older in [reply(405, ""), reply(404, r#"{"error":"Nicht gefunden"}"#)] {
            assert_eq!(
                answered(remove(&me, "f1").unwrap(), older).unwrap(),
                Social::ServerTooOld
            );
        }
        let gone = "Diese Freundschaft oder Anfrage gibt es nicht";
        assert_eq!(
            answered(
                remove(&me, "f1").unwrap(),
                reply(404, &format!(r#"{{"error":"{gone}"}}"#))
            )
            .unwrap_err()
            .to_string(),
            gone
        );
        assert_eq!(
            answered(remove(&me, "f1").unwrap(), reply(401, "{}")).unwrap(),
            Social::SignInAgain
        );
    }

    #[test]
    fn a_ranking_marks_the_own_entry_only_among_friends_and_only_when_its_name_is_unique() {
        let me = state(Some(true));
        let rows = |body: &str, friends: bool| {
            let Social::Ready(list) =
                answered(ranking(&me, friends).unwrap(), reply(200, body)).unwrap()
            else {
                panic!("ready")
            };
            list
        };
        let one = r#"{"entries":[{"display_name":"Ada","xp":120,"ref":"n1"},{"display_name":"Lea","xp":80,"ref":"n2"},{"display_name":"Cleo","xp":80,"ref":"n3"},{"display_name":"Dan","xp":5,"ref":"n4"}]}"#;
        let friends = rows(one, true);
        let marked: Vec<(u32, bool)> = friends
            .entries
            .iter()
            .map(|row| (row.place, row.own))
            .collect();
        assert_eq!(marked, [(1, false), (2, true), (2, false), (4, false)]);
        assert_eq!(friends.entries[0].reference.as_deref(), Some("n1"));
        assert_eq!(friends.entries[0].points, 120);
        assert!(!friends.without_you);
        // Everyone: a name tells nobody apart, so nothing is marked (a stranger named Lea keeps
        // its report button).
        let everyone = rows(one, false);
        assert!(everyone.entries.iter().all(|row| !row.own));
        // A friend with the same name: neither is marked.
        let two = r#"{"entries":[{"display_name":"Lea","xp":3,"ref":"n1"},{"display_name":"Lea","xp":0,"ref":"n2"}]}"#;
        assert!(rows(two, true).entries.iter().all(|row| !row.own));
        let absent = rows(
            r#"{"entries":[{"display_name":"Ada","xp":1,"ref":"n1"}]}"#,
            false,
        );
        assert!(absent.without_you);
        let call = first(&mut ranking(&me, true).unwrap());
        assert!(call.url.ends_with("/v1/ranking?scope=friends"));
        let none = answered(ranking(&me, false).unwrap(), reply(402, PAY)).unwrap();
        assert_eq!(none, Social::NotForAccount);
        no_subscription_words(&none);
        assert_eq!(
            answered(ranking(&me, false).unwrap(), reply(404, "")).unwrap(),
            Social::ServerTooOld
        );
    }

    #[test]
    fn a_report_counts_once_the_address_is_confirmed() {
        let run = |verified: Option<bool>, replies: Vec<Reply>| {
            let mut flow = report(&state(verified), "n1", Some("  ")).unwrap();
            let call = first(&mut flow);
            assert_eq!(call.body, Some(Body::Json(br#"{"ref":"n1"}"#.to_vec())));
            let mut calls = 1;
            for reply in replies {
                match flow.step(Some(reply)).unwrap() {
                    Step::Done(output) => return (output, calls),
                    Step::Call(call) => {
                        assert!(call.url.ends_with("/v1/me"), "{}", call.url);
                        calls += 1;
                    }
                    Step::Calls(_) => panic!("one call at a time"),
                }
            }
            panic!("not done")
        };
        let ready = |counts_now| Social::Ready(Reported { counts_now });
        assert_eq!(run(Some(true), vec![reply(204, "")]), (ready(true), 1));
        assert_eq!(run(None, vec![reply(204, "")]), (ready(true), 1));
        let account = |verified: bool| {
            reply(
                200,
                &format!(
                    r#"{{"id":"me","email":"lea@example.org","display_name":"Lea","subscription":"active","plan":null,"period_end":null,"email_verified":{verified}}}"#
                ),
            )
        };
        assert_eq!(
            run(Some(false), vec![reply(204, ""), account(true)]),
            (ready(true), 2)
        );
        assert_eq!(
            run(Some(false), vec![reply(204, ""), account(false)]),
            (ready(false), 2)
        );
        assert_eq!(
            run(Some(false), vec![reply(204, ""), reply(503, "")]),
            (ready(false), 2)
        );
        // The account check got no reply (status 0): the report is done all the same.
        assert_eq!(
            run(Some(false), vec![reply(204, ""), reply(0, "")]),
            (ready(false), 2)
        );
        assert_eq!(
            run(Some(true), vec![reply(405, "")]),
            (Social::ServerTooOld, 1)
        );
        assert_eq!(
            run(Some(true), vec![reply(404, "")]),
            (Social::ServerTooOld, 1)
        );
        let me = state(Some(true));
        assert!(report(&me, "n1", Some(&"x".repeat(301))).is_err());
        assert!(report(&me, "", None).is_err());
        let mut with = report(&me, "n1", Some(" rude ")).unwrap();
        assert_eq!(
            first(&mut with).body,
            Some(Body::Json(br#"{"ref":"n1","reason":"rude"}"#.to_vec()))
        );
    }

    /// A server on a local port that answers each request with `answer(request line)` (an empty
    /// body is a 204), or closes the connection without a reply where that is `None` (a lost
    /// network, a server gone away).
    fn local_server(answer: fn(&str) -> Option<&'static str>) -> String {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let server = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for mut socket in listener.incoming().flatten() {
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(&socket);
                    let mut request = String::new();
                    reader.read_line(&mut request).unwrap();
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        if line == "\r\n" || line.is_empty() {
                            break;
                        }
                        if let Some((name, value)) = line.split_once(':')
                            && name.eq_ignore_ascii_case("content-length")
                        {
                            length = value.trim().parse().unwrap();
                        }
                    }
                    reader.read_exact(&mut vec![0; length]).unwrap();
                    if let Some(body) = answer(&request) {
                        let status = if body.is_empty() {
                            "204 No Content"
                        } else {
                            "200 OK"
                        };
                        let reply = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        socket.write_all(reply.as_bytes()).unwrap();
                    }
                });
            }
        });
        server
    }

    #[test]
    fn on_the_desktop_a_lost_second_call_ends_as_in_the_browser() {
        // The report goes through; the account check after it gets no reply. The report is done
        // and counts once the address is confirmed.
        let mut me = state(Some(false));
        me.server = local_server(|request| (!request.starts_with("GET /v1/me ")).then_some(""));
        assert_eq!(
            cloud_http::run_answering(report(&me, "n1", None).unwrap()).unwrap(),
            Social::Ready(Reported { counts_now: false })
        );
        // The friend list comes, the link list does not: the page shows the friends.
        let mut me = state(Some(true));
        me.server = local_server(|request| {
            (!request.starts_with("GET /v1/friends/invites "))
                .then_some(r#"[{"display_name":"Ada","accepted":true,"id":"f1"}]"#)
        });
        let Social::Ready(page) = cloud_http::run_answering(friends(&me).unwrap()).unwrap() else {
            panic!("ready")
        };
        assert_eq!(page.friends.len(), 1);
        assert_eq!(
            page.invites,
            InviteList::Failed("Locora Cloud nicht erreichbar".into())
        );
        // Without the friend list there is no page.
        me.server =
            local_server(|request| (!request.starts_with("GET /v1/friends ")).then_some("[]"));
        assert_eq!(
            cloud_http::run_answering(friends(&me).unwrap())
                .unwrap_err()
                .to_string(),
            "Locora Cloud nicht erreichbar"
        );
    }

    #[test]
    fn invite_codes_are_read_before_the_network() {
        let me = state(Some(true));
        for junk in [
            "",
            "hello",
            "ABCD-EFGH",
            &format!("{}#ABCD-EFGH-JKLM-NPQR", "x".repeat(600)),
        ] {
            assert_eq!(
                preview_invite(&me, junk)
                    .err()
                    .map(|e| e.to_string())
                    .as_deref(),
                Some(NO_CODE),
                "{junk}"
            );
            assert!(accept_invite(&me, junk).is_err());
        }
        // The message the app shares with a code but no link is read as it was received, in
        // every language and with a full stop a messenger may add.
        for message in [
            "Lass uns in Locora Freunde sein! Gib meinen Einladungscode in Locora unter „Freunde & Ranglisten“ ein (gilt bis 7. November 2026): ABCD-EFGH-JKMN-PQRS",
            "Let's be friends in Locora! Enter my invite code in Locora under “Friends & rankings” (valid until November 7, 2026): abcd-efgh-jkmn-pqrs.",
            "Soyons amis dans Locora ! Saisis mon code d’invitation dans Locora, sous « Amis et classements » (valable jusqu’au 7 novembre 2026)\u{202f}: ABCD-EFGH-JKMN-PQRS",
            "Locora でフレンドになろう！Locora の「フレンドとランキング」で私の招待コードを入力してね（2026年11月7日 まで有効）：ABCD-EFGH-JKMN-PQRS",
            "我们在 Locora 里做好友吧！请在 Locora 的“好友与排行榜”中输入我的邀请码（有效期至 2026年11月7日）：ABCD-EFGH-JKMN-PQRS",
        ] {
            let mut flow = preview_invite(&me, message).unwrap();
            assert_eq!(
                first(&mut flow).url,
                "https://api.example.com/v1/friends/invites/ABCD%2DEFGH%2DJKMN%2DPQRS",
                "{message}"
            );
            assert!(accept_invite(&me, message).is_ok());
        }
        // A shared link with a long query is still read (its code is the fragment).
        let tracked = format!(
            "https://web.locora.cloud/einladung?utm={}#abcd-efgh-jkmn-pqrs",
            "x".repeat(200)
        );
        let mut flow = preview_invite(&me, &tracked).unwrap();
        let call = first(&mut flow);
        assert_eq!(
            call.url,
            "https://api.example.com/v1/friends/invites/ABCD%2DEFGH%2DJKMN%2DPQRS"
        );
        assert_eq!(call.token, None, "a preview is asked without the session");
        let from = answered(
            preview_invite(&me, "ABCD EFGH JKMN PQRS").unwrap(),
            reply(200, r#"{"display_name":"Ada","expires_at":7,"ref":"n9"}"#),
        )
        .unwrap();
        assert_eq!(
            from,
            Social::Ready(InviteFrom {
                code: "ABCD-EFGH-JKMN-PQRS".into(),
                name: "Ada".into(),
                expires_at: 7,
                reference: Some("n9".into()),
            })
        );
        assert_eq!(
            answered(
                preview_invite(&me, "ABCD-EFGH-JKMN-PQRS").unwrap(),
                reply(404, r#"{"error":"Nicht gefunden"}"#)
            )
            .unwrap(),
            Social::ServerTooOld
        );
        let own =
            "Das ist deine eigene Einladung. Schick den Link der Person, die du einladen möchtest.";
        assert_eq!(
            answered(
                accept_invite(&me, "ABCD-EFGH-JKMN-PQRS").unwrap(),
                reply(400, &format!(r#"{{"error":"{own}"}}"#))
            )
            .unwrap_err()
            .to_string(),
            own
        );
        let mut accept = accept_invite(&me, "ABCD-EFGH-JKMN-PQRS").unwrap();
        let call = first(&mut accept);
        assert_eq!(call.token.as_deref(), Some("t"));
        assert!(call.url.ends_with("/accept"));
        let none = answered(
            accept_invite(&me, "ABCD-EFGH-JKMN-PQRS").unwrap(),
            reply(402, PAY),
        )
        .unwrap();
        assert_eq!(none, Social::NotForAccount);
        no_subscription_words(&none);
        let friend = answered(
            accept_invite(&me, "ABCD-EFGH-JKMN-PQRS").unwrap(),
            reply(200, r#"{"display_name":"Ada","accepted":true,"id":"f7"}"#),
        )
        .unwrap();
        assert_eq!(
            friend,
            Social::Ready(FriendEntry {
                id: Some("f7".into()),
                name: "Ada".into()
            })
        );
        let full = answered(
            create_invite(&me).unwrap(),
            reply(409, r#"{"error":"Es sind schon 10 Einladungslinks offen"}"#),
        )
        .unwrap_err();
        assert!(
            full.to_string()
                .starts_with("Du hast die Höchstzahl offener Einladungslinks")
        );
        assert_eq!(
            answered(create_invite(&me).unwrap(), reply(405, "")).unwrap(),
            Social::ServerTooOld
        );
        let none = answered(create_invite(&me).unwrap(), reply(402, PAY)).unwrap();
        no_subscription_words(&none);
        let created = answered(
            create_invite(&me).unwrap(),
            reply(
                201,
                r#"{"code":"ABCD-EFGH-JKLM-NPQR","url":"https://evil.example/pay","expires_at":9}"#,
            ),
        )
        .unwrap();
        assert_eq!(
            created,
            Social::Ready(InviteLink {
                code: "ABCD-EFGH-JKLM-NPQR".into(),
                url: None,
                expires_at: 9
            })
        );
        assert_eq!(
            answered(
                revoke_invite(&me, "ABCD-EFGH-JKLM-NPQR").unwrap(),
                reply(404, r#"{"error":"Nicht gefunden"}"#)
            )
            .unwrap(),
            Social::ServerTooOld
        );
        assert_eq!(
            answered(
                revoke_invite(&me, "ABCD-EFGH-JKLM-NPQR").unwrap(),
                reply(204, "")
            )
            .unwrap(),
            Social::Ready(())
        );
        assert!(revoke_invite(&me, "junk").is_err());
    }

    #[test]
    fn the_states_travel_as_the_ui_reads_them() {
        let ready: Social<()> = Social::Ready(());
        let text = serde_json::to_string(&ready).unwrap();
        assert_eq!(text, r#"{"state":"ready","value":null}"#);
        assert_eq!(serde_json::from_str::<Social<()>>(&text).unwrap(), ready);
        assert_eq!(
            serde_json::to_string(&Social::<()>::NotForAccount).unwrap(),
            r#"{"state":"not_for_account"}"#
        );
    }
}
