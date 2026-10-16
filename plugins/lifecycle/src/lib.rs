//! Mutations committed during failed activation and shutdown must still notify subscribers.
use locora_sdk::prelude::*;

#[locora_plugin(permissions = [palaces_read, palaces_write])]
struct Lifecycle;
impl Plugin for Lifecycle {
    fn activate(ctx: &mut Ctx) -> Result {
        for mode in ["activation-error", "activation-trap"] {
            if let Ok(palaces::Response::Palace(p)) =
                ctx.palaces(&palaces::Request::Get { query: mode.into() })
            {
                ctx.palaces(&palaces::Request::Delete { id: p.id })?;
                if mode == "activation-trap" {
                    panic!("Startabsturz");
                }
                return Err(Error::msg("Startfehler"));
            }
        }
        Ok(())
    }
    fn deactivate(ctx: &mut Ctx) {
        if let Ok(palaces::Response::Palace(p)) = ctx.palaces(&palaces::Request::Get {
            query: "deactivation".into(),
        }) {
            ctx.palaces(&palaces::Request::Delete { id: p.id }).unwrap();
        }
    }
}
