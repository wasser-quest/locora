use locora_sdk::prelude::*;

#[locora_plugin(name = {{name_literal}}, permissions = [storage])]
struct {{type}};

impl Plugin for {{type}} {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.on::<JourneyCompleted>(|ctx, ev| {
            let journeys = ctx.store().add("journeys", 1)?;
            ctx.info(format!(
                "Journey in {} beendet: {}/{} richtig (insgesamt {journeys} Journeys)",
                ev.palace_id, ev.correct, ev.total
            ));
            Ok(())
        })
    }
}
