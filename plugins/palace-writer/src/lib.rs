//! Capability and filter-chain integration fixture.
use locora_sdk::prelude::*;
#[path = "../../test_support.rs"]
mod test_support;

#[locora_plugin(permissions = [palaces_write])]
struct PalaceWriter;
impl Plugin for PalaceWriter {
    fn activate(ctx: &mut Ctx) -> Result {
        test_support::activate(ctx, "palace-writer")
    }
}
