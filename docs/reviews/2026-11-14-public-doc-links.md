# Review-Protokoll: docs/public-doc-links

Datum: 2026-11-14
Ablage: `docs/reviews/2026-11-14-public-doc-links.md` (Handbuch 29.4)

## Gegenstand

| Feld | Wert |
|------|------|
| Geprüfter Commit | 81e5d8c677910426fe3b18f44e15e2b7c3959c70, auf dem Remote-Branch `docs/public-doc-links` erhalten |
| Integrationsbasis | `main` @ f389aa0 |
| Umfang | gesamter Diff `main..81e5d8c`: Doc-Kommentar von `Board::open` in `src/device/mod.rs` |
| Anlass | N4 aus `2026-11-14-split-device-by-platform.md`: Die öffentliche Doku verwies auf das private Modul `crate::lock`, `cargo doc` mit `-D warnings` brach ab |
| Kriterien | Regelwerk der Repositories, Handbuch Abschnitte 13 und 33; ein Befund blockiert bei einer falschen Aussage im geänderten Text |
| Kontextübergabe (Pflicht) | frische Session ohne Autorenkontext; Eingaben: Auftrag, Diff, Regelwerk |
| Prüfwerkzeug | nicht im Repo geführt (Repo-Hygiene), Ablage: ykur-rs-2026-11-14-doc |
| Integration | Der Squash-Commit nennt den geprüften Commit und dieses Protokoll (33.4); zum geprüften Stand kommt nur diese Datei hinzu (29.4) |

## Prüfer: Rolle und Art

| Rolle | Art | Name | Grad der Unabhängigkeit |
|-------|-----|------|-------------------------|
| Code-Reviewer | Werkzeug-Review | - | getrennte Session, Eingaben wie oben |

Ein Werkzeug-Review ist eine Vorprüfung, kein Nachweis einer unabhängigen
Bewertung. Die Art sagt nichts über die Kontextübergabe aus.

## Urteil

ACCEPT für 81e5d8c; keine Findings der Schwere Medium oder höher (Reviewer-Ausgabe). Der neue
Text stimmt mit dem Code und der Moduldoku von `src/lock.rs` überein: advisory `flock` auf dem
hidraw-Knoten unter Linux, exklusives Öffnen unter macOS, ein Open je Knoten unter FreeBSD. Das
Urteil autorisiert weder Merge noch Release.

## Findings (aus der Reviewer-Ausgabe)

| ID | Beschreibung | Datei:Zeile | Status | Blockierend |
|---|---|---|---|---|
| L1 | Die CI hat kein Gate für `cargo doc` mit `-D warnings`; ein neuer Verweis auf ein privates Element fiele wieder erst bei einem manuellen Lauf auf | `.github/workflows/ci.yml` | bestätigt, eigener Branch vorgesehen | nein |
| I2 | Die öffentliche Doku nennt nicht, dass ein Programm ohne Lock unter Linux nicht aufgehalten wird und dass unter macOS ein Programm, das das Board schon offen hatte, seine Writes verliert; beides stand auch vorher nur in der privaten Moduldoku | `src/device/mod.rs:107-109` | bestätigt | nein |
| I3 | Der Begriff "turn" wird in der öffentlichen Doku nicht eingeführt | `src/device/mod.rs:107` | bestätigt | nein |
| I4 | "another invocation" ist enger als das Verhalten: auch ein zweites Öffnen im selben Prozess wartet; die Formulierung bestand schon vorher | `src/device/mod.rs:106` | bestätigt | nein |

Die Einstufung "nicht blockierend" ist die des Reviews, Entscheidung der zuständigen Rolle offen.

## Gate-Ergebnisse

Exit-Codes aus dem tatsächlichen Lauf, am Stand 81e5d8c:

| Gate | Kommando | Exit-Code |
|------|----------|-----------|
| Doku | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked`, Host und `--target aarch64-unknown-linux-gnu`, je auch mit `--document-private-items` | 0 |
| Doku FreeBSD | `RUSTDOCFLAGS="-D warnings" cargo +1.98 doc --no-deps --locked --target aarch64-unknown-freebsd`, auch mit `--document-private-items` | 0 |
| Gegenprobe | dasselbe öffentlich auf `main` | 101 |
| Tests | `cargo test --locked` (76, 24, 9, 6 und 1 Doctest bestanden, 7 ignoriert) | 0 |
| Lint | `cargo clippy --all-targets --locked -- -D warnings` | 0 |
| Format | `cargo fmt --check` | 0 |
| Text-Gate | `check_text.py --root <worktree> --commits main..81e5d8c` und ohne `--commits` (je 0 Findings) | 0 |
| CI | Lauf 43609811180 an 81e5d8c, alle 6 Jobs | success |

## Nicht verifiziert

| Punkt | Markierung |
|---|---|
| Verhalten von FreeBSD-hidraw (ein Open je Knoten) und `Access::Exclusive` unter macOS: an keiner Primärquelle geprüft; der Text folgt Code und Moduldoku, die Hardware-Läufe vom 2026-11-14 stützen es | Grenze |

## Abweichungen vom Ablauf

Keine.
