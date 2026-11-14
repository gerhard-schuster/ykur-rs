# Review-Protokoll: refactor/split-device-by-platform

Datum: 2026-11-14
Ablage: `docs/reviews/2026-11-14-split-device-by-platform.md` (Handbuch 29.4)

## Gegenstand

| Feld | Wert |
|------|------|
| Geprüfter Commit | 2b6affaabbb998d845279a257ba566c62d05dc98, auf dem Remote-Branch `refactor/split-device-by-platform` erhalten |
| Integrationsbasis | `main` @ 126571d |
| Umfang | gesamter Diff `main..2b6affa`: Aufteilung von `src/device.rs` in `src/device/` (`mod.rs`, `waiting.rs`, `nodes.rs`, `linux.rs`, `macos.rs`, `freebsd.rs`) und ein Satz im Abschnitt Coverage des README |
| Anspruch der Änderung | reine Umstrukturierung ohne geändertes Verhalten auf macOS, Linux und FreeBSD |
| Kriterien | Regelwerk der Repositories, Handbuch Abschnitte 13 und 33; ein Befund blockiert bei einem Defekt im geänderten Code oder einer falschen Aussage in geändertem Text |
| Kontextübergabe (Pflicht) | frische Session ohne Autorenkontext; Eingaben: Auftrag, Diff, alter Stand von `src/device.rs`, Regelwerk, Nachweis-Logs. Das Nachreview zu 2b6affa stammt von derselben prüfenden Rolle und hatte das erste Urteil als Kontext |
| Prüfwerkzeug | nicht im Repo geführt (Repo-Hygiene), Ablage: ykur-rs-2026-11-14 |
| Integration | Der Squash-Commit nennt den geprüften Commit und dieses Protokoll (33.4); zum geprüften Stand kommt nur diese Datei hinzu (29.4) |

## Prüfer: Rolle und Art

| Rolle | Art | Name | Grad der Unabhängigkeit |
|-------|-----|------|-------------------------|
| Code-Reviewer | Werkzeug-Review | - | getrennte Session, Eingaben wie oben |

Ein Werkzeug-Review ist eine Vorprüfung, kein Nachweis einer unabhängigen
Bewertung. Die Art sagt nichts über die Kontextübergabe aus.

## Urteil

ACCEPT für 7ffe5d9 (Code), danach ACCEPT für den gesamten Diff `main..2b6affa` nach dem
README-Nachtrag zu Finding F1; keine offenen blockierenden Findings (Reviewer-Ausgabe). Das
Review hat die Zeilen des alten und der neuen Dateien verglichen: Jede abweichende Zeile ist eine
Verschiebung, eine neue Sichtbarkeit (`pub(super)`), ein Modulkopf, ein Import oder gehört zur
Umwandlung der drei `cfg`-Blöcke von `Board::open_within` in je ein `open()` pro Plattform.
Reihenfolge von Lock und Öffnen unter Linux, Drop-Reihenfolge der Felder von `Board`, die
öffentliche API und die auf jedem System gebauten Tests sind unverändert. Das Urteil autorisiert
weder Merge noch Release.

## Findings (aus der Reviewer-Ausgabe)

| ID | Beschreibung | Datei:Zeile | Status | Blockierend |
|---|---|---|---|---|
| F1 | Der Abschnitt Coverage nannte als Ort nicht erreichter Zeilen weiter `device.rs`, das es nicht mehr gibt | `README.md:196-207` | behoben in 2b6affa; Nachreview: jede Aussage des neuen Satzes an 44d21e9, 126571d und 2b6affa belegt, historische Zahlen unverändert | nein mehr |
| N1 | Die vollen Testpfade haben sich geändert: drei Tests liegen unter `device::nodes::tests`, `only_busy_from_the_transport_counts_as_a_board_in_use` und die sieben Wartetests unter `device::waiting::tests`. Die Kurznamen sind gleich. Im Repo verweisen nur eingefrorene Nachweise auf die alten Pfade | `src/device/nodes.rs`, `src/device/waiting.rs` | bestätigt | nein |
| N2 | Der Kommentar "dropped last" an `_turn` ist für `Turn = ()` auf macOS und FreeBSD gegenstandslos; der Kommentar sagt das selbst | `src/device/mod.rs:96-100` | bestätigt | nein |
| N3 | Der Testhelfer `os_error` steht doppelt | `src/device/mod.rs:552`, `src/device/waiting.rs:134` | bestätigt | nein |
| N4 | `cargo doc` mit `-D warnings` bricht ab: die öffentliche Doku von `Board::open` verweist auf das private Modul `crate::lock`. Auf `main` gleich, durch die Änderung nicht verschlimmert | `src/device/mod.rs:107` | bestätigt, vorbestehend | nein |
| N5 | Ob der Zweig für ein verweigertes Open in `error.rs` heute noch unerreicht ist, sagt der README-Satz nicht; vermutlich erreicht ihn inzwischen ein Unit-Test | `src/error.rs:138-163` | `UNKNOWN`, für die nächste Coverage-Messung | nein |

Die Einstufung "nicht blockierend" ist die des Reviews, Entscheidung der zuständigen Rolle offen.

## Gate-Ergebnisse

Exit-Codes aus dem tatsächlichen Lauf. 2b6affa ändert gegenüber 7ffe5d9 nur `README.md`; die
Läufe an 7ffe5d9 gelten für den Code.

| Gate | Kommando | Exit-Code |
|------|----------|-----------|
| Tests macOS | `cargo test --locked` (76, 24, 9, 6 und 1 Doctest bestanden, 7 ignoriert) | 0 |
| Testliste | `cargo test --locked -- --list --include-ignored`: Kurznamen gleich wie auf `main` (macOS 123, Linux 129, FreeBSD 123) | gleich |
| Lint | `cargo clippy --all-targets --locked -- -D warnings` | 0 |
| Lint Linux | `cargo clippy --all-targets --locked --target aarch64-unknown-linux-gnu -- -D warnings` | 0 |
| Lint FreeBSD | `cargo +1.98 clippy --all-targets --locked --target aarch64-unknown-freebsd -- -D warnings` | 0 |
| MSRV | `cargo +1.98 check --all-targets --locked`, auch mit `--target aarch64-unknown-freebsd` | 0 |
| Format | `cargo fmt --check` | 0 |
| Lieferkette | `cargo deny --all-features check`; `Cargo.toml` und `Cargo.lock` nicht im Diff | 0 |
| Text-Gate | `check_text.py --root <worktree> --commits main..2b6affa` und ohne `--commits` (je 0 Findings) | 0 |
| Doku | `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`: auf `main` und am Branch derselbe Fehler (N4) | 101 / 101 |
| CI | Lauf 43608986888 an 2b6affa, alle 6 Jobs; Tests macOS 76, 24, 9, 6 und 1 Doctest, Linux 82, 24, 9, 6 und 1 Doctest bestanden | success |
| Tests Linux | Debian-VM, Linux aarch64: `cargo test --locked` und clippy für `main` und 7ffe5d9 | 0 |
| Tests FreeBSD | FreeBSD-15.1-VM, arm64: `cargo test --locked` und clippy für `main` und 7ffe5d9; erstmals laufen die Tests auf FreeBSD, CI übersetzt dort nur | 0 |
| Hardware | `cargo test --locked -- --ignored --test-threads=1`, Board `YK22170`, Firmware 1.2.1, für `main` und 7ffe5d9: macOS nativ, Debian-VM und FreeBSD-VM je 7 bestanden, 0 fehlgeschlagen | 0 |
| Programm am Board | `ykurctrl -l` findet `YK22170`, `-g everything` meldet alle Ausgänge aus; Debian-VM und FreeBSD-VM, `main` und 7ffe5d9 | 0 |

Der Hardware-Test `a_held_board_is_not_opened_a_second_time` deckt auf jedem System den eigenen
Open-Pfad ab: `flock` unter Linux, das exklusive Öffnen unter macOS, `NodeWait` mit dem
Einzel-Open unter FreeBSD.

## Nicht verifiziert

| Punkt | Markierung |
|---|---|
| Warten eines zweiten Aufrufs, bis der erste fertig ist: am Board nicht geprüft, nur die sofortige Ablehnung mit Wartezeit null | Grenze |
| Abziehen im Betrieb mit Exit 69 auf allen drei Systemen | `UNKNOWN` |
| Die macOS-Hardware-Logs nennen keinen Commit; der Stand ist in der Ablage vermerkt | Grenze |
| Rote TDD-Phase: für eine reine Umstrukturierung nicht zu erwarten | Grenze |

## Abweichungen vom Ablauf

Keine.

## Altlasten außerhalb dieser Änderung

Der Doku-Verweis auf das private Modul (N4) und die offenen Findings des Protokolls
`2026-11-09-align-with-behaviour.md`. Dieselbe Aufteilung steht für ykush-rs aus, dort als
eigener Branch.
