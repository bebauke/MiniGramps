# MiniGramps Performance

## In Arbeit

- [x] Beziehungsindex pro Layoutdurchlauf statt wiederholter linearer Familien-/Personensuche
- [x] Kartenmaße nur für sichtbare Personen und sichtbare Partner berechnen
- [x] Viewport-Culling und vereinfachte Karten bei kleinem Zoom
- [x] Personenliste nur bei Daten-/Sortieränderungen neu gruppieren
- [x] Detaillierte Layoutdiagnose im Release-Build deaktivieren oder schaltbar machen
- [x] Unveränderliche Arbeit in `repel_pass` aus den Iterationen herausziehen

## Danach

- [ ] Baumlayout von der Darstellung trennen und per Revisionsschlüssel cachen
- [ ] Partner-Tausch: Zieh-Gestus einmalig auslösen (Latch), kein Hin-und-her-Flickern
- [x] Zentrale Frame-Kopien von `expanded` und `manual_offsets` entfernen
- [x] Baumkarten: erste beiden Vornamen plus Nachname; im großen Foto klein drunter
- [x] Zoom-Zentrum am Mauszeiger und konfigurierbarer Baum-Abstand (Default doppelt)
- [x] Profilbild füllt die Karte bei ausgeblendeten Inhaltsdetails (kleiner Zoom)
- [ ] Fotos vollständig asynchron dekodieren und GPU-Uploads pro Frame begrenzen
- [ ] Undo-Historie von vollständigen `TreeData`-Kopien auf Änderungsbefehle umstellen
- [ ] Einstellungen dauerhaft speichern

## Messung

- [ ] Frame-Zeit für Layout, Zeichnung, Personenliste und Medien separat messen
- [ ] Vergleich mit dem 963-Personen-Projekt in `cargo run --release`
