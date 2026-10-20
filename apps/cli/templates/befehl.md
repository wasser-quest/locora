# Befehl-Plugin

Diese Vorlage registriert den Textdienst `{{id}}.command@1`. Passe die Funktion in
`src/lib.rs` an. Zum Testen:

```bash
locora plugin dev --once
locora plugin install target/wasm32-wasip2/release/{{id}}.wasm --yes
locora plugin invoke {{id}} command "Hallo"
```

`plugin invoke` ruft ein installiertes Plugin im aktuellen Profil auf. Die
Befehlspalette der App hat noch kein Plugin-Register; dieser Dienst erscheint
dort derzeit nicht automatisch.
