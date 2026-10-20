# Locus-Typ-Prototyp

Diese Vorlage registriert `{{id}}.locus@1` und prüft `Soll|Ist` mit Locoras
Textnormalisierung. Zum Testen:

```bash
locora plugin dev --once
locora plugin install target/wasm32-wasip2/release/{{id}}.wasm --yes
locora plugin invoke {{id}} locus "Erde|erde"
```

Journeys unterstützen derzeit die Antwortarten Recall, Quiz und Guess. Eine
neue Abfrageart braucht zusätzlich eine versionierte Antwort-Schnittstelle,
Speicherung, Bewertung und einen Renderer in der App. Der Dienst dieser Vorlage
wird noch nicht automatisch in Journeys aufgerufen.
