use locora_sdk::prelude::*;

#[locora_plugin(name = "Lernzeit", permissions = [storage])]
struct Lernzeit;

impl Plugin for Lernzeit {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<JourneyCompleted>(|ctx, ev| {
            let seconds = ctx.store().add("seconds", ev.duration_secs)?;
            ctx.info(format!("+{} s für {}", ev.duration_secs, ev.palace_id));
            ctx.tile("Lernzeit", format!("{} min", seconds / 60))
        })
    }
}
