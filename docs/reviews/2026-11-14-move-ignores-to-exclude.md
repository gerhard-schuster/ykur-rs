# Review-Protokoll: chore/move-ignores-to-exclude

Datum: 2026-11-14
Ablage: `docs/reviews/2026-11-14-move-ignores-to-exclude.md` (Handbuch 29.4)

## Gegenstand

| Feld | Wert |
|------|------|
| Geprüfter Commit | ed7ab58ca385fedf65b7bd087b02ea5dcdb23497, auf dem Remote-Branch `chore/move-ignores-to-exclude` erhalten |
| Integrationsbasis | `main` @ 29f33a2 |
| Umfang | gesamter Diff `main..ed7ab58`: Block mit den Namen der lokalen Anweisungsdateien aus `.gitignore` entfernt |
| Anlass | Entscheidung des Rechteinhabers vom 2026-11-14: die getrackte `.gitignore` wird veröffentlicht und würde die Werkzeuge nennen; die Einträge stehen in der lokalen `.git/info/exclude` jedes Klons. Das Regelwerk ist entsprechend geändert. |
| Kriterien | Regelwerk der Repositories, Handbuch Abschnitte 13 und 33 |
| Kontextübergabe (Pflicht) | frische Session ohne Autorenkontext; Eingaben: Auftrag, Diff, lokale Exclude-Datei, Regelwerk |
| Prüfwerkzeug | nicht im Repo geführt (Repo-Hygiene), Ablage: ykur-rs-2026-11-14-exclude |
| Integration | Der Squash-Commit nennt den geprüften Commit und dieses Protokoll (33.4); zum geprüften Stand kommt nur diese Datei hinzu (29.4) |

## Prüfer: Rolle und Art

| Rolle | Art | Name | Grad der Unabhängigkeit |
|-------|-----|------|-------------------------|
| Code-Reviewer | Werkzeug-Review | - | getrennte Session, Eingaben wie oben |

Ein Werkzeug-Review ist eine Vorprüfung, kein Nachweis einer unabhängigen
Bewertung. Die Art sagt nichts über die Kontextübergabe aus.

## Urteil

ACCEPT für ed7ab58, ohne Findings (Reviewer-Ausgabe). Das Review machte den Merge von der
Änderung des Regelwerks abhängig; sie ist seit dem 2026-11-14 in Kraft. Das Urteil autorisiert
weder Merge noch Release.

## Restrisiko (aus der Reviewer-Ausgabe)

Ein frischer Klon ohne die Einträge in `.git/info/exclude` ignoriert die Dateien nicht. Das
fangen das Stagen nur mit Namen (G1) und das Gate ab, das jede getrackte Anweisungsdatei meldet,
sofern die Hooks laufen oder CI das Gate ausführt. Die Einrichtung eines Klons beschreibt das
Regelwerk. In der Historie des Repos bleibt die frühere `.gitignore` sichtbar.

## Gate-Ergebnisse

| Gate | Kommando | Exit-Code |
|------|----------|-----------|
| Ignorieren | `git check-ignore -v` für alle zehn Einträge, je auch eine Datei in den beiden Verzeichnissen der Liste: über die lokale Exclude-Datei; `worktrees/x` über `.gitignore:4` | 0 |
| Getrackte ignorierte Dateien | `git ls-files -ci --exclude-standard` (leer) | 0 |
| Werkzeugnamen im Baum | `git grep -i` nach den Namen am Stand ed7ab58 (kein Treffer) | 1 |
| Text-Gate | `check_text.py --root <worktree> --commits main..ed7ab58` und ohne `--commits` (je 0 Findings) | 0 |
| CI | Lauf 43613567790 an ed7ab58, alle 6 Jobs | success |

## Abweichungen vom Ablauf

Keine.
