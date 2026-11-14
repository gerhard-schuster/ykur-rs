# Review-Protokoll: ci/check-public-docs

Datum: 2026-11-14
Ablage: `docs/reviews/2026-11-14-check-public-docs.md` (Handbuch 29.4)

## Gegenstand

| Feld | Wert |
|------|------|
| Geprüfter Commit | 8df91fe5e19ce3ec5882a1439cfdc9d158505197, auf dem Remote-Branch `ci/check-public-docs` erhalten |
| Integrationsbasis | `main` @ 29f33a2 |
| Umfang | gesamter Diff `main..8df91fe`: zwei Schritte im Job `lint` von `.github/workflows/ci.yml`, die die öffentliche Doku mit `RUSTDOCFLAGS=-D warnings` bauen (Host und `aarch64-unknown-freebsd`), und die Zeile dazu in `CONTRIBUTING.md` |
| Anlass | L1 aus `2026-11-14-public-doc-links.md`: ein Verweis der öffentlichen Doku auf ein privates Element fiel nur bei einem manuellen Lauf auf |
| Art der Änderung | Verschärfung der CI (G8) |
| Kriterien | Regelwerk der Repositories, Handbuch Abschnitte 13 und 33 |
| Kontextübergabe (Pflicht) | frische Session ohne Autorenkontext; Eingaben: Auftrag, Diff, Regelwerk |
| Prüfwerkzeug | nicht im Repo geführt (Repo-Hygiene), Ablage: ykur-rs-2026-11-14-doc-gate |
| Integration | Der Squash-Commit nennt den geprüften Commit und dieses Protokoll (33.4); zum geprüften Stand kommt nur diese Datei hinzu (29.4) |

## Prüfer: Rolle und Art

| Rolle | Art | Name | Grad der Unabhängigkeit |
|-------|-----|------|-------------------------|
| Code-Reviewer | Werkzeug-Review | - | getrennte Session, Eingaben wie oben |

Ein Werkzeug-Review ist eine Vorprüfung, kein Nachweis einer unabhängigen
Bewertung. Die Art sagt nichts über die Kontextübergabe aus.

## Urteil

ACCEPT für 8df91fe; keine Findings der Schwere Medium oder höher (Reviewer-Ausgabe). Offen war
nur der CI-Lauf; er ist grün, und beide neuen Schritte liefen auf beiden Runnern (siehe
Gate-Ergebnisse). Das Urteil autorisiert weder Merge noch Release.

## Findings (aus der Reviewer-Ausgabe)

| ID | Beschreibung | Datei:Zeile | Status | Blockierend |
|---|---|---|---|---|
| L1 | `SECURITY-REVIEW-0.1.0.md` zählt auf, was `ci.yml` prüft, ohne den neuen Schritt; datiertes Protokoll, keine Änderung empfohlen | `SECURITY-REVIEW-0.1.0.md:33`, `:208` | bestätigt | nein |
| L2 | Die Zeile in `CONTRIBUTING.md` prüft nur den Host, CI zusätzlich FreeBSD; wie die Zeile für clippy daneben | `CONTRIBUTING.md:7` | bestätigt | nein |
| I1 | Der Kommentar nennt nur private Verweise; `-D warnings` macht jede rustdoc-Warnung zum Fehler | `.github/workflows/ci.yml:89-90` | bestätigt | nein |

Die Einstufung "nicht blockierend" ist die des Reviews, Entscheidung der zuständigen Rolle offen.

## Gate-Ergebnisse

| Gate | Kommando | Exit-Code |
|------|----------|-----------|
| Doku | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`, Host und `--target aarch64-unknown-linux-gnu` | 0 |
| Doku FreeBSD | `RUSTDOCFLAGS="-D warnings" cargo +1.98 doc --no-deps --locked --target aarch64-unknown-freebsd` | 0 |
| YAML | `ruby -ryaml -e 'YAML.load_file(ARGV[0])' .github/workflows/ci.yml` | 0 |
| Text-Gate | `check_text.py --root <worktree> --commits main..8df91fe` und ohne `--commits` (je 0 Findings) | 0 |
| CI | Lauf 43613564910 an 8df91fe, alle 6 Jobs; in beiden `lint`-Jobs liefen beide `cargo doc`-Schritte mit Erfolg | success |

## Nicht verifiziert

| Punkt | Markierung |
|---|---|
| Wirksamkeit: dass der Schritt bei einem eingebauten Doku-Fehler rot wird, ist nicht vorgeführt; der Fehler auf `f389aa0` (Exit 101 lokal) zeigt die Klasse, die er fängt | Grenze |

## Abweichungen vom Ablauf

Keine.
