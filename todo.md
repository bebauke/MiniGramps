# MiniGramps Performance

## In Arbeit

- [x] Beziehungsindex pro Layoutdurchlauf statt wiederholter linearer Familien-/Personensuche
- [x] Kartenmaße nur für sichtbare Personen und sichtbare Partner berechnen
- [x] Viewport-Culling und vereinfachte Karten bei kleinem Zoom
- [x] Personenliste nur bei Daten-/Sortieränderungen neu gruppieren
- [x] Detaillierte Layoutdiagnose im Release-Build deaktivieren oder schaltbar machen

## Danach

- [ ] Baumlayout von der Darstellung trennen und per Revisionsschlüssel cachen
- [ ] Unveränderliche Arbeit in `repel_pass` aus den Iterationen herausziehen
- [x] Zentrale Frame-Kopien von `expanded` und `manual_offsets` entfernen
- [ ] Fotos vollständig asynchron dekodieren und GPU-Uploads pro Frame begrenzen
- [ ] Undo-Historie von vollständigen `TreeData`-Kopien auf Änderungsbefehle umstellen
- [ ] Einstellungen dauerhaft speichern

## Messung

- [ ] Frame-Zeit für Layout, Zeichnung, Personenliste und Medien separat messen
- [ ] Vergleich mit dem 963-Personen-Projekt in `cargo run --release`
