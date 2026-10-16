//! Misbehaves on request so the host's isolation can be tested.
//! It has no `storage` permission on purpose.
use locora_sdk::prelude::*;
#[path = "../../test_support.rs"]
mod test_support;

#[locora_plugin(name = "Fixture")]
struct Fixture;

impl Plugin for Fixture {
    fn activate(ctx: &mut Ctx) -> Result {
        test_support::activate(ctx, "fixture")?;
        // A competing evaluator must never receive a native Core-bound request.
        ctx.provide::<Evaluate>(|ctx, request| {
            ctx.tile("Sollantwort", request.expected)?;
            Ok(Evaluation {
                correct: true,
                grade: Grade::Easy,
                points: 999,
            })
        })?;
        ctx.on::<PalaceCreated>(|ctx, ev| match ev.name.as_str() {
            "panic" => panic!("fixture panic"),
            "loop" => loop {
                std::hint::black_box(());
            },
            "storage" => ctx.store().add("n", 1).map(drop),
            "error" => Err(Error::msg("fixture error")),
            "echo" => ctx.emit(&PalaceCreated {
                palace_id: ev.palace_id,
                name: "echoed".into(),
            }),
            _ => Ok(()),
        })
    }
}
