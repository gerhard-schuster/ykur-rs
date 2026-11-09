# Review-Protokoll: docs/align-with-behaviour

Datum: 2026-11-09
Ablage: `docs/reviews/2026-11-09-align-with-behaviour.md` (Handbuch 29.4)

## Gegenstand

| Feld | Wert |
|------|------|
| Geprüfter Commit | 1ef452a322afe5961464b874b73efe267217084d, auf dem Remote-Branch `docs/align-with-behaviour` erhalten |
| Integrationsbasis | `main` @ 8bc38f5e7aa5ad5d3028961363c8f7a1e0cb6a3d |
| Umfang | gesamter Diff `origin/main..1ef452a`: Code, Tests, CI, Doku, Manpage, Prüfprotokoll |
| Kriterien | Regelwerk der Repositories, Handbuch Abschnitte 13 und 33; ein Befund blockiert bei einem Defekt im geänderten Code oder einer falschen oder widersprüchlichen Aussage in geändertem Text |
| Kontextübergabe (Pflicht) | frische Session ohne Autorenkontext; die Neubeurteilung von 1ef452a stammt von derselben prüfenden Rolle wie das Urteil zu d56ec07 und hatte dessen Befunde als Kontext |
| Prüfwerkzeug | nicht im Repo geführt (Repo-Hygiene), Ablage: ykur-rs-2026-11-09 |
| Integration | Der Squash-Commit nennt den geprüften Commit und dieses Protokoll (33.4); zum geprüften Stand kommt nur diese Datei hinzu (29.4) |

## Prüfer: Rolle und Art

| Rolle | Art | Name | Grad der Unabhängigkeit |
|-------|-----|------|-------------------------|
| Code-Reviewer | Werkzeug-Review | - | getrennte Session, Eingaben: Auftrag, Diff, Regelwerk, Quellen |

Ein Werkzeug-Review ist eine Vorprüfung, kein Nachweis einer unabhängigen
Bewertung. Die Art sagt nichts über die Kontextübergabe aus.

## Urteil

ACCEPT für den Stand 1ef452a, gesamter Diff gegen `origin/main`; keine offenen blockierenden
Findings (Reviewer-Ausgabe). Das Review machte das ACCEPT von einem grünen CI-Lauf am Endstand
abhängig, einschließlich `test (ubuntu-24.04-arm)` mit 121 Tests; diese Bedingung ist erfüllt
(siehe Gate-Ergebnisse). Das Urteil autorisiert weder Merge noch Release. Der Squash ist vom
Rechteinhaber am 2026-11-09 beauftragt.

Das vorhergehende Urteil zu d56ec07 lautete CHANGE_REQUIRED (F1, F2). Die Umbenennung von `all`
in `everything` hat der Rechteinhaber entschieden.

## Findings (unverändert aus der Reviewer-Ausgabe)

| ID | Beschreibung | Datei:Zeile | Status | Blockierend |
|---|---|---|---|---|
| F1 | `all` hatte in `ykurcmd` eine andere Bedeutung, siehe oben | `src/cli.rs:90-93`, `README.md:69-73`, `README.md:264` | behoben, aus den Quelltexten bestaetigt | nein mehr |
| F2 | Kommentar gegen Verhalten bei `ENOENT`, siehe oben | `src/lock.rs:190-199` | behoben, aus dem Code bestaetigt, nicht ausgefuehrt | nein mehr |
| N1 | Ein waehrend des Wartens offenes `ENXIO` gilt in `NodeWait` nicht als `Gone`. Das Warten bricht in einem engen Fenster mit 69 ab. Die Logik bestand schon vorher. | `src/device.rs:393-402` | plausibel, nicht gemessen | nein |
| N2 | `From<HidError>` erkennt nur `std::io::Error`, nicht den vom Transport verpackten `nix::Errno`. Scheitert das ioctl auf FreeBSD wegen eines verschwindenden Boards, ergibt das 74. Lesen und Schreiben sind nicht betroffen. | `src/error.rs:150-161` | plausibel, enges Fenster | nein |
| N3 | Die fruehere Abweichung der macOS-Coverage-Zeile ist **erledigt**: Nachmessung an 1ef452a ergibt genau 89,69 %, 182 von 1766. Der Zusatz zu angeschlossenen Boards passt, denn `tests/cli.rs:65` erreicht die Enumeration. Der Wert haengt damit von der Umgebung ab, wie die Zeile sagt. | `README.md:192` | bestaetigt | nein |
| N4 | Die Zeilen zu L2 und "Report sizes" nennen FreeBSD nicht. Das ist unvollstaendig, nicht falsch. | `SECURITY-REVIEW-0.1.0.md:29`, `:81`, `:252` | bestaetigt | nein |
| N5 | Die Manpage nennt unter 69 weniger Faelle als das README, `hidraw(4)` fehlt unter SEE ALSO, und dass `all` abgelehnt wird, steht nicht darin. | `man/ykurctrl.1:113-115`, `:140-142` | bestaetigt | nein |
| N6 | Der Kommentar zu `ENODEV` nennt keine Quelle, und die Doku von `From` ist fuer Linux unvollstaendig: Lesen liefert `EIO`. Die Abbildung selbst stimmt. | `src/error.rs:8`, `:139-144` | bestaetigt | nein |
| N7 | Alle Tests sind Entwicklertests des Autors, unabhaengige Verifikationstests fehlen, und das Repo hat kein Requirements-Dokument. Die Fehlerpfade laufen nur ueber konstruierte Fehlerwerte. | `src/error.rs`, `src/lock.rs`, `src/cli.rs` | bestaetigt | nein |
| N8 | Das Praefix `docs/` passt nicht zu den Aenderungen an Code, Tests und CI (33.1). Der Squash beseitigt den Namen. | Branchname | bestaetigt | nein |
| N9 | Die CI meldet die Abkuendigung von Node.js 20 fuer die gepinnten Actions. | `.github/workflows/ci.yml` | bestaetigt | nein |
| N10 | Keine CI an 1ef452a und kein Lauf des geaenderten Linux-Tests. Die Aussagen im Protokoll zum CI-Lauf "dieser Aenderung" stuetzen sich bisher auf den Lauf an d56ec07, nicht auf den Endstand. | `src/lock.rs:752-762`, `SECURITY-REVIEW-0.1.0.md:36`, `:72` | bestaetigt; `gh run list` liefert fuer den Commit keinen Lauf | nein, aber Voraussetzung fuer den Merge |
| N11 | Fuer `ENOENT` entfaellt der Knotenpfad in der Meldung. Fehlt der Knoten, obwohl das Board da ist (etwa ein Container ohne `/dev`-Eintrag), heisst es "no longer connected" statt "Cannot open". Exit 69 passt zur README-Zeile "no hidraw node", nur der Text fuehrt in die Irre. | `src/lock.rs:195-199` | plausibel, nicht ausgefuehrt | nein |
| N12 | `ykurcmd` wertet nur das erste Zeichen aus, also auch andere Werte (`-d relay`, `-u 1x`). `ykurctrl` lehnt sie mit Exit 64 ab. Die Tabelle nennt nur `-d all` und das Modul heisst weiter "compatible". Das ist unkritisch, weil die Abweichung laut fehlschlaegt und nichts unbemerkt schaltet. | `src/cli.rs:2`, `README.md:258-270` | aus dem Quelltext bestaetigt | nein |

Stand nach dem Review: N10 ist durch den CI-Lauf 42656009071 an 1ef452a erledigt; darin lief der
geänderte Linux-Test mit. Die übrigen offenen Findings sind nicht entschieden: Die Einstufung
"nicht blockierend" ist die des Reviews, Entscheidung der zuständigen Rolle offen.

## Gate-Ergebnisse

Exit-Codes aus dem tatsächlichen Lauf, am Endstand 1ef452a:

| Gate | Kommando | Exit-Code |
|------|----------|-----------|
| Tests | `cargo test --all-features --locked` (115 und 1 Doctest, 7 ignoriert) | 0 |
| Lint | `cargo clippy --all-targets --all-features --locked -- -D warnings` | 0 |
| Lint Linux | `cargo clippy --all-targets --locked --target aarch64-unknown-linux-gnu -- -D warnings` | 0 |
| MSRV | `cargo +1.98 check --all-targets --locked`, auch mit `--target aarch64-unknown-freebsd` | 0 |
| Lint FreeBSD | `cargo +1.98 clippy --all-targets --locked --target aarch64-unknown-freebsd -- -D warnings` | 0 |
| Format | `cargo fmt --check` | 0 |
| Lieferkette | `cargo deny --all-features check` | 0 |
| Manpage | `mandoc -T lint -W warning man/ykurctrl.1` | 0 |
| Text-Gate | `check_text.py --root <worktree> --commits origin/main..1ef452a` (0 Findings) | 0 |
| Abdeckung | `cargo test --locked` instrumentiert: 89,69 %, 182 von 1766 | 0 |
| CI | Lauf 42656009071, alle 6 Jobs; Linux 82, 24, 9, 6 und 1 Doctest bestanden (121 Tests) | success |
| Hardware | `cargo test --locked --no-fail-fast -- --ignored --test-threads=1`, macOS, Board `YK22170`, Firmware 1.2.1: 7 bestanden, 0 fehlgeschlagen | 0 |
| Programm am Board | `ykurctrl -u all`: Exit 64, nichts geschaltet; `-u everything`: Exit 0, vier Ports und Relais an; `-d everything`: Exit 0, alles aus; Ausgänge davor und danach aus | wie angegeben |

Der Hardware-Lauf lief auch an d56ec07 mit demselben Ergebnis.

## Nicht verifiziert

| Punkt | Markierung |
|---|---|
| Linux und FreeBSD am Board: nicht gelaufen; Unplug-Pfade (`ENODEV`, `ENOENT`, `ENXIO`) und die FreeBSD-Schreibgrenze nur aus dem Quelltext; FreeBSD-Tests laufen auch in CI nicht | `UNKNOWN` |
| Abziehen im Betrieb mit Exit 69: auf keinem System gemessen | `UNKNOWN` |
| Interrupt-OUT-Endpunkt des Boards | `UNKNOWN` |
| Entfernung des Geräteknotens beim Abziehen unter Linux | `MISSING_SOURCE` |
| `ykurcmd`: nur der Quelltext am Stand ae4a095 gelesen, nicht ausgeführt | Grenze |
| Rote TDD-Phase: keine Laufnachweise | Grenze |

## Abweichungen vom Ablauf

- Drei Commits des Branches entstanden am 2026-11-07 direkt auf dem lokalen `main`, ohne
  vorheriges Review (G3). Sie waren nie auf dem Remote; `main` wurde am 2026-11-09 auf
  `origin/main` zurückgesetzt und die Commits auf dem Branch weitergeführt.
- Der Branch wurde zunächst im Hauptverzeichnis bearbeitet statt in einem Worktree (Handbuch
  33.2 Nr. 6). Mit Zustimmung des Rechteinhabers ging es am 2026-11-09 zurück auf `main`, der
  Rest entstand im Worktree.

## Offene Punkte vor einem Release

1. Hardware-Tests und Abziehen im Betrieb auf Linux (Raspberry Pi) und FreeBSD am aktuellen
   Stand; die gepinnte Transport-Revision lief auf dem Pi noch nicht.
2. M1-Versuch (verwaiste Antwort) auf Linux.
3. Die Revision des Transports ist nur über einen Branch des Forks erreichbar; Tag setzen oder
   auf crates.io umstellen, sobald die Änderung veröffentlicht ist.

## Altlasten außerhalb dieser Änderung

Requirements und Trennung der Tests (N7), `ENXIO` beim Warten auf FreeBSD (N1), `nix::Errno` im
FreeBSD-ioctl (N2), die Meldung bei fehlendem Knoten (N11) und die weiteren Findings N4 bis N6,
N9 und N12.
