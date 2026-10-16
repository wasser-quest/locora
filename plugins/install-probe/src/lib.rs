//! Installation must inspect this component without running activation.
use locora_sdk::__bindings::{
    self as bindings,
    locora::plugin::{host, types},
};
use locora_sdk::prelude::*;

struct InstallProbe;
impl bindings::Guest for InstallProbe {
    fn manifest() -> types::Manifest {
        // Even manifest code must not gain access to the profile.
        assert!(host::kv_set("manifest", b"bad").is_err());
        let request = locora_sdk::api::encode(&palaces::Request::Create {
            name: "manifest-probe".into(),
        })
        .unwrap();
        assert!(host::call_host("core.palaces@1", &request).is_err());
        let mut manifest = locora_sdk::__rt::manifest(
            "install-probe",
            "Installationsprobe",
            "0.1.0",
            &["storage", "palaces:write"],
        );
        // Distinct string lets tests patch only the manifest API, not WIT imports.
        manifest.api = "0.4.123".into();
        manifest
    }
    fn activate() -> std::result::Result<(), String> {
        host::kv_set("activated", b"bad")?;
        panic!("Installationsprobe darf nicht aktiviert werden");
    }
    fn deactivate() {}
    fn on_event(_: String, _: Vec<u8>) -> std::result::Result<(), String> {
        Ok(())
    }
    fn on_call(_: String, _: Vec<u8>) -> std::result::Result<Vec<u8>, String> {
        Ok(vec![])
    }
    fn on_filter(_: String, _: Vec<u8>) -> std::result::Result<Vec<u8>, String> {
        Ok(vec![])
    }
}
bindings::__export_plugin!(InstallProbe);
