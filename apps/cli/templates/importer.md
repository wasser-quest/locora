# Textimporter-Plugin

Diese Vorlage registriert `{{id}}.import@1`. Die Eingabedatei besteht aus einem
Palastnamen und 1 bis 100 Zeilen `Ort | Inhalt`:

```text
Mein Palast
Tür | Merkur
Fenster | Venus
```

Zum Testen:

```bash
locora plugin dev --once
locora plugin install target/wasm32-wasip2/release/{{id}}.wasm --yes
locora plugin invoke {{id}} import --file daten.txt
```

Der CLI-Host liest höchstens 1 MiB UTF-8 und übergibt nur den Text. Das Plugin
hat `palaces:write`, aber keinen Dateizugriff. Es legt einen neuen Palast mit
einem leeren Raum an. Bei einem Hostfehler nach der Anlage können bereits
geschriebene Loci bestehen bleiben; ein atomarer Import ist noch nicht Teil der
Palast-Schnittstelle.
