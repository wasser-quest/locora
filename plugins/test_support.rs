//! Shared integration-test protocol for real WASM fixtures.
use locora_sdk::prelude::*;

thread_local! {
    static SEEN_EVENT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct PalaceProxy;
impl Service for PalaceProxy {
    const NAME: &'static str = "fixture.palaces@1";
    type Request = palaces::Request;
    type Response = palaces::Response;
}

struct RawProxy;
impl Service for RawProxy {
    const NAME: &'static str = "fixture.raw@1";
    type Request = (String, Vec<u8>);
    type Response = Vec<u8>;
}

pub fn activate(ctx: &mut Ctx, id: &'static str) -> Result {
    ctx.on::<Scored>(|ctx, event| ctx.tile("Punkte", event.points.to_string()))?;
    ctx.provide::<RawProxy>(|_, (service, bytes)| {
        locora_sdk::__bindings::locora::plugin::host::call_host(&service, &bytes)
            .map_err(Error::msg)
    })?;
    if id != "fixture" {
        ctx.on::<PalaceCreated>(|ctx, ev| {
            SEEN_EVENT.set(true);
            ctx.tile("Test", ev.name)
        })?;
    }
    ctx.provide::<PalaceProxy>(|ctx, request| ctx.palaces(&request))?;
    ctx.filter::<JourneyScore>(
        if id == "fixture" { -10 } else { 0 },
        move |ctx, mut score| {
            if score.palace_id == "event" && SEEN_EVENT.get() {
                score.points = 99;
            }
            if id == "fixture" {
                match score.palace_id.as_str() {
                    "error" => return Err(Error::msg("Filterfehler")),
                    "duplicate" => ctx.filter::<JourneyScore>(99, |_, score| Ok(score))?,
                    "panic" => panic!("Filterabsturz"),
                    "loop" => loop {
                        std::hint::black_box(());
                    },
                    "event" => ctx.emit(&PalaceCreated {
                        palace_id: "p".into(),
                        name: "echo".into(),
                    })?,
                    _ => {}
                }
                score.points += 1;
            } else if id == "palace-reader" {
                score.points *= 2;
            } else {
                if score.palace_id == "writer-error" {
                    return Err(Error::msg("Später Filterfehler"));
                }
                score.points += 3;
            }
            Ok(score)
        },
    )
}
