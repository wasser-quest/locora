use locora_sdk::prelude::*;

#[locora_plugin(name = {{name_literal}}, permissions = [storage])]
struct {{type}};

impl Plugin for {{type}} {
    fn activate(ctx: &mut Ctx) -> Result {
        let richtig: u64 = ctx.store().get("richtig")?.unwrap_or(0);
        ctx.tile({{name_literal}}, format!("{richtig} richtig"))?;

        ctx.on::<JourneyCompleted>(|ctx, ev| {
            let richtig = ctx.store().add("richtig", ev.correct.into())?;
            ctx.tile({{name_literal}}, format!("{richtig} richtig"))
        })
    }
}
