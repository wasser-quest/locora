//! Capability and filter-chain integration fixture.
use locora_sdk::prelude::*;
#[path = "../../test_support.rs"]
mod test_support;

#[locora_plugin(permissions = [palaces_read])]
struct PalaceReader;
impl Plugin for PalaceReader {
    fn activate(ctx: &mut Ctx) -> Result {
        test_support::activate(ctx, "palace-reader")
    }
}
