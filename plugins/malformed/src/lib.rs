//! Deliberately bypass the SDK to test host response validation and reserved services.
use locora_sdk::__bindings::{
    self as bindings,
    locora::plugin::{host, types},
};

struct Malformed;
impl bindings::Guest for Malformed {
    fn manifest() -> types::Manifest {
        locora_sdk::__rt::manifest("malformed", "Test", "0.1.0", &[])
    }
    fn activate() -> Result<(), String> {
        host::provide("core.palaces@1");
        host::register_filter("core.journey.score", 5)
    }
    fn deactivate() {}
    fn on_event(_: String, _: Vec<u8>) -> Result<(), String> {
        Ok(())
    }
    fn on_call(_: String, _: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(vec![])
    }
    fn on_filter(_: String, _: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(vec![])
    }
}
bindings::__export_plugin!(Malformed);
