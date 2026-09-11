# MiniGramps – Roadmap

Offene Punkte zuerst, danach der erledigte Stand als Nachweis.

## Offen

### Gramps-Daten & Import

- [ ] Vollständiger Gramps-XML-Import: weitere Namensbestandteile (Rufname,
      Präfix/Suffix, Titel, mehrere Namen)
- [ ] Kindesart aus Gramps übernehmen (`mrel`/`frel`: leiblich, adoptiert,
      Stief-, Pflegekind)
- [ ] Quellen/Zitate, Notizen, Medien/Galerie, Attribute/Tags und Orts-Objekte
- [ ] Export nach GEDCOM und Gramps-XML (Export-Dialog ist derzeit Platzhalter)

### Layout & Darstellung

- [ ] Nachfahrenbaum ab 4 Generationen stabilisieren: Gruppen reißen auseinander
      (Diagnose vorhanden: `RUST_LOG=minigramps=debug` + `F9`, Zeilen `STAGE_*`,
      `NEG_MOVE`, `LAYOUT_GAP`, `LAYOUT_FAMILY`)
- [ ] Baumlayout von der Darstellung trennen und per Revisionsschlüssel cachen
      (kein kompletter Neuaufbau pro Frame)
- [ ] Fenstergröße/-position in den globalen Einstellungen merken

### Performance

- [ ] Fotos vollständig asynchron dekodieren und GPU-Uploads pro Frame begrenzen
- [ ] Undo-Historie von vollständigen `TreeData`-Kopien auf Änderungsbefehle umstellen
- [ ] Frame-Zeit für Layout, Zeichnung, Personenliste und Medien separat messen
- [ ] Vergleich mit dem 963-Personen-Projekt in `cargo run --release`

### Plattformen

- [ ] Android: nativer Runner, Speicher und Datei-/Medien-Picker
- [ ] Web/WASM: echtes Laden/Speichern statt reiner In-Memory-Demodaten

### Qualität

- [ ] Testabdeckung ausbauen (Layout, Import-Sonderfälle, Store)
- [ ] Lizenz festlegen (derzeit keine hinterlegt)

## Erledigt

### Baum & Interaktion

- [x] Maus-Zoom auf Zeigerposition; konfigurierbarer Baum-Abstand als Default (30–150)
- [x] Fester Mindestabstand zwischen Karten (`MIN_CARD_GAP`)
- [x] Referenz-Historie mit Zurück/Vor-Navigation (2×2-Cluster unter Undo/Redo)
- [x] Partner-Tausch per Zieh-Geste einmalig auslösen (Latch, kein Flickern)
- [x] Datumsnormierung auf Karten (`DD Mnt YYYY`) inkl. numerischer Eingabeformate
- [x] Rufnamen-Regel: erster abweichender Vorname + Rufname + Nachname
- [x] Profilbild bei ausgeblendeten Details eingepasst (nicht gestreckt); ohne Foto Initialen
- [x] Horizontale Großansicht: Karten nicht mehr auf 215px gestreckt (Basis 160)
- [x] Ansichts-Schalter responsiv (STAMMBAUM aus, Kürzel Nachf./Vorf./Fächer/V/H mit Tooltips)

### Panels & Persistenz

- [x] Globale Einstellungen in `settings.json` (automatisch persistiert)
- [x] Referenzperson pro Projekt im Layout-File speichern/laden
- [x] Personenliste: Suche/Filter mit Auto-Expand; beim Leeren alle wieder einklappen
- [x] Zahnrad öffnet Flyout mit Sortierung (Gruppengröße/Alphabet)
- [x] Einstellungen dauerhaft speichern

### Fenster & Diagnose

- [x] Rahmenlose Rand-Resizerkennung (`BeginResize`) und 5px-Fensterrundung (`SetWindowRgn`)
- [x] Logging auf Level umgestellt (`log`/`env_logger`, `RUST_LOG`); App `info`, Diagnose `debug`
- [x] Gestufte Layout-Dumps (`STAGE_*`) und große Einzelverschiebungen (`NEG_MOVE`)

### Frühere Performance-Arbeit

- [x] Beziehungsindex pro Layoutdurchlauf statt wiederholter linearer Suche
- [x] Kartenmaße nur für sichtbare Personen/Partner berechnen
- [x] Viewport-Culling und vereinfachte Karten bei kleinem Zoom
- [x] Personenliste nur bei Daten-/Sortieränderungen neu gruppieren
- [x] Unveränderliche Arbeit in `repel_pass` aus den Iterationen herausziehen
- [x] Zentrale Frame-Kopien von `expanded` und `manual_offsets` entfernen
