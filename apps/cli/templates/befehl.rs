//! CLI command service: `locora plugin invoke {{id}} command "Text"`.
//! A command-palette contribution needs a separate app UI registry.
use locora_sdk::prelude::*;

#[locora_plugin(name = {{name_literal}}, permissions = [])]
struct {{type}};

struct Run;
impl Service for Run {
    const NAME: &'static str = "{{id}}.command@1";
    type Request = String;
    type Response = String;
}

impl Plugin for {{type}} {
    fn activate(ctx: &mut Ctx) -> Result {
        ctx.provide::<Run>(|_ctx, text| {
            let text = text.trim();
            if text.is_empty() {
                return Err(Error::msg("Bitte Text eingeben"));
            }
            Ok(format!("{}: {text}", {{name_literal}}))
        })
    }
}
