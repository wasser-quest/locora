//! Experimental answer service: `locora plugin invoke {{id}} locus "Soll|Ist"`.
//! The journey UI currently supports Recall, Quiz and Guess only. Integrating
//! a new question renderer requires a versioned answer ABI and app UI work.
use locora_sdk::prelude::*;

#[locora_plugin(name = {{name_literal}}, permissions = [])]
struct {{type}};

struct Check;
impl Service for Check {
    const NAME: &'static str = "{{id}}.locus@1";
    type Request = String;
    type Response = String;
}

impl Plugin for {{type}} {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Check>(|_ctx, input| {
            let (expected, given) = input
                .split_once('|')
                .ok_or_else(|| Error::msg("Erwartet Soll|Ist"))?;
            if expected.trim().is_empty() {
                return Err(Error::msg("Sollwert fehlt"));
            }
            let correct = normalize(expected) == normalize(given);
            Ok(if correct { "richtig" } else { "falsch" }.to_owned())
        })
    }
}
