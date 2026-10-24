//! Friends, invite links, rankings and name reports, off the UI thread. They do not take the sync
//! lock: none of them changes the profile. The flows, and the checks of their arguments, are
//! `locora_app_core::social`, which the browser build runs too.

use locora_app_api::{
    FriendAsked, FriendEntry, FriendsList, InviteFrom, InviteLink, RankingList, Reported, Social,
};
use locora_app_core::social;
use locora_runtime::cloud::{self, CloudState};
use locora_runtime::cloud_http::{self, Flow};
use tauri::Manager;

use crate::{Shared, err, lock};

/// Runs one friends flow for the saved account; without one the answer is `SignedOut`, and
/// nothing is sent.
async fn run<T, F>(
    app: &tauri::AppHandle,
    make: impl FnOnce(&CloudState) -> anyhow::Result<F> + Send + 'static,
) -> Result<Social<T>, String>
where
    F: Flow<Output = Social<T>>,
    T: Send + 'static,
{
    let data = {
        let state = app.state::<Shared>();
        lock(&state)?.data.clone()
    };
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = cloud::load(&data).map_err(err)? else {
            return Ok(Social::SignedOut);
        };
        cloud_http::run(make(&state).map_err(err)?).map_err(err)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn cloud_friends(app: tauri::AppHandle) -> Result<Social<FriendsList>, String> {
    run(&app, social::friends).await
}

#[tauri::command]
pub async fn cloud_friend_ask(
    app: tauri::AppHandle,
    email: String,
) -> Result<Social<FriendAsked>, String> {
    run(&app, move |state| social::ask(state, &email)).await
}

#[tauri::command]
pub async fn cloud_friend_remove(app: tauri::AppHandle, id: String) -> Result<Social<()>, String> {
    run(&app, move |state| social::remove(state, &id)).await
}

#[tauri::command]
pub async fn cloud_ranking(
    app: tauri::AppHandle,
    friends: bool,
) -> Result<Social<RankingList>, String> {
    run(&app, move |state| social::ranking(state, friends)).await
}

#[tauri::command]
pub async fn cloud_report(
    app: tauri::AppHandle,
    reference: String,
    reason: Option<String>,
) -> Result<Social<Reported>, String> {
    run(&app, move |state| {
        social::report(state, &reference, reason.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn cloud_invite_create(app: tauri::AppHandle) -> Result<Social<InviteLink>, String> {
    run(&app, social::create_invite).await
}

#[tauri::command]
pub async fn cloud_invite_revoke(
    app: tauri::AppHandle,
    code: String,
) -> Result<Social<()>, String> {
    run(&app, move |state| social::revoke_invite(state, &code)).await
}

#[tauri::command]
pub async fn cloud_invite_preview(
    app: tauri::AppHandle,
    code: String,
) -> Result<Social<InviteFrom>, String> {
    run(&app, move |state| social::preview_invite(state, &code)).await
}

#[tauri::command]
pub async fn cloud_invite_accept(
    app: tauri::AppHandle,
    code: String,
) -> Result<Social<FriendEntry>, String> {
    run(&app, move |state| social::accept_invite(state, &code)).await
}
