# Tray-Aufnahmefenster — Vorschlag, noch nicht implementiert

## Ziel

Ein Klick auf das Tray-Symbol öffnet eine kleine Aufnahmeansicht nahe dem Tray: Space startet/stoppt, P pausiert, der Status zeigt Verarbeitung und die Fertig-Bestätigung kommt erst nach der Ausgabe. Das Hauptfenster bleibt für Einstellungen und History erreichbar.

## Empfohlener Ablauf

1. Tray-Klick öffnet/fokussiert die kompakte Ansicht ohne automatisch aufzunehmen.
2. Action und Ausgabe übernehmen die zuletzt gespeicherten Einstellungen; Start, Pause und Stop nutzen denselben nativen Aufnahmezustand wie das Hauptfenster.
3. Nach Stop bleibt „Processing“ sichtbar. Der native Fertigton bestätigt die erfolgreiche Verarbeitung/Ausgabe; ein Clipboard-Häkchen erscheint nur bei tatsächlich erfolgreichem Kopieren.
4. Fertigmeldung kurz stehen lassen, anschliessend per Escape/Klick ausserhalb ausblenden. Fehler bleiben sichtbar und der Text bleibt kopierbar.
5. Ein klarer „Open Utterform“-Eintrag öffnet das Hauptfenster; Tray-Quit bleibt ein expliziter App-Exit.

## Vor Umsetzung entscheiden

- Soll Tray-Klick standardmässig den Popup oder weiterhin das Hauptfenster öffnen? Empfehlung: Popup mit dauerhaft erreichbarem Hauptfenster-Eintrag.
- Soll der Popup nach Erfolg automatisch verschwinden? Empfehlung: zunächst manuell schliessen, damit Fehler und längere Verarbeitung verständlich bleiben.

## Technische Grenzen

Tray-Geometrie und Aktivierung unterscheiden sich zwischen macOS, Windows und Linux/Wayland. Insbesondere AppIndicator unter Linux bietet nicht überall einen verlässlichen Linksklick oder Koordinaten; dort braucht es einen Menüeintrag und eine sinnvolle Ersatzposition. Das Positionieren direkt neben dem Tray darf deshalb nicht plattformübergreifend versprochen werden.

Ein zweites WebView darf keine zweite Aufnahme/Verarbeitung auslösen. Vor Umsetzung den gemeinsamen nativen Session-/Verarbeitungszustand und Event-Synchronisierung für beide Ansichten definieren; Fokusverlust darf eine Aufnahme nicht abbrechen. Keine globalen Space-Hotkeys, die andere Apps stören. Autostart ist ein separater, ausdrücklich aktivierbarer Wunsch und wird mit 0.3.1 nicht heimlich eingerichtet.
