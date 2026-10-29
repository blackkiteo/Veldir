# Veldir v0.3.0 — file manager leggero per Windows

Prototipo **open source MIT** in Rust + egui, ispirato alla filosofia di File Pilot, con codice e identità indipendenti. **Non affiliato a File Pilot.**

> **Stato reale:** Veldir 0.2.0 e 0.2.1 hanno superato compilazione, test e build release su GitHub Actions. La versione 0.3.0 introduce un aggiornamento dell'interfaccia; scaricare l'EXE solo da una build 0.3.0 completata con successo. **Non è ancora stato eseguito un collaudo manuale della GUI Windows. Non usare su file importanti prima di quel collaudo.**

## Novità della v0.3.0

- **Pannelli ridimensionabili** trascinando il divisore centrale. La proporzione viene salvata e ripristinata.
- **Barra laterale richiudibile** per dedicare tutto lo spazio ai file; lo stato resta memorizzato.
- **Percorso a segmenti:** clicca una delle cartelle nella barra degli indirizzi per raggiungerla; Ctrl+L consente ancora di digitare un percorso completo.
- **Ordinamento interattivo:** clicca Nome, Dimensione o Modificato per ordinare, clicca di nuovo per invertire (cartelle sempre in alto).
- **Selezione rapida:** Ctrl+A seleziona gli elementi visibili nel filtro, Esc annulla la selezione.
- Maggior spazio tra i controlli, colori e indicatori di stato rifiniti.

## Correzioni v0.2.1

- Focus stabile dei campi indirizzo e ricerca: le scorciatoie Ctrl+L e Ctrl+F puntano a widget con ID persistenti.
- Cambiando o azzerando il filtro vengono deselezionati tutti gli elementi, per evitare operazioni su file nascosti dal filtro.
- Le scorciatoie da tastiera non cambiano cartelle o selezioni mentre è aperta una finestra di conferma.

## Funzionalità introdotte nella v0.2

- **Doppio pannello:** attivabile dalla barra superiore; il pannello di destra naviga in modo indipendente. La barra delle schede riguarda il pannello di sinistra.
- **Selezione multipla:** clic per selezionare, Ctrl+clic per aggiungere/rimuovere, Maiusc+clic per selezionare un intervallo nell'elenco filtrato. Rinomina disponibile con un solo elemento selezionato.
- **Copia e spostamento multipli:** clipboard interna (Ctrl+C, Ctrl+X, Ctrl+V) e bottoni «⇢ Copia» / «⇢ Sposta» verso il pannello opposto. Conferma per lo spostamento tramite il bottone.
- **Drag & drop in ingresso da Windows:** i file trascinati nella finestra di Veldir aprono una richiesta di conferma, con scelta del pannello di destinazione. Per sicurezza il rilascio **copia**, non sposta.
- **Cestino multiplo:** elimina nel Cestino solo dopo conferma. **Non è presente un comando di eliminazione permanente.**
- **Sicurezza operazioni batch:** controlli preventivi dei conflitti, protezione di ogni singolo trasferimento da sovrascritture; una modifica concorrente del filesystem può comunque interrompere un batch dopo aver completato alcuni elementi.
- **Sessione persistente:** pannello secondario e percorso salvati insieme alle schede e ai preferiti.

## Funzioni confermate nei sorgenti precedenti

Tema scuro, schede, unità, preferiti, scorciatoie, navigazione con cronologia, filtro rapido locale, apertura di file con il programma predefinito, nuova cartella, rinomina, spostamento su stesso disco, copia streaming con staging temporaneo e protezione dai symlink/junction. Elenchi letti su thread separati con virtualizzazione delle righe.

## Compilazione su Windows 10/11 x64

1. Installa Rust 1.95+ con toolchain **MSVC** (<https://rustup.rs/>) e i Visual Studio C++ Build Tools.
2. Estrai lo ZIP senza perdere le cartelle `.github`, `src` e `scripts`.
3. In PowerShell, dalla cartella contenente `Cargo.toml`, esegui:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows.ps1
```

Lo script invoca `cargo fmt --all`, `cargo check --all-targets`, `cargo test --all-targets` e `cargo build --release`, fermandosi a qualsiasi errore. Solo dopo un completamento riuscito troverai `target\release\veldir.exe`.

### GitHub Actions

Carica i file in **un repository Veldir dedicato**. In **Actions → Veldir Windows Build** viene compilata la versione Windows e, se tutti i controlli passano, viene pubblicato come artefatto `Veldir-v0.3.0-Windows-x64` con l'EXE e il suo SHA-256. La build v0.2.0 è stata verificata nella CI; **il collaudo manuale dell'interfaccia resta necessario**.

## Scorciatoie

| Comando | Tasti |
| --- | --- |
| Nuova scheda / chiudi scheda sinistra | Ctrl+T / Ctrl+W |
| Selezione aggiuntiva / intervallo / tutti i visibili | Ctrl+clic / Maiusc+clic / Ctrl+A |
| Passa tra i pannelli | F6 (apre il secondo se disattivato) |
| Modifica percorso / filtro pannello attivo | Ctrl+L / Ctrl+F |
| Indietro / avanti / cartella superiore | Alt+← / Alt+→ / Alt+↑ |
| Aggiorna entrambi i pannelli e le schede | F5 |
| Clipboard interna (copia / taglia / incolla) | Ctrl+C / Ctrl+X / Ctrl+V |
| Deseleziona / Rinomina uno / Cestino multiplo | Esc / F2 / Canc |
| Apri un elemento | Invio / doppio clic |

## Limitazioni ancora aperte

- **Collaudo GUI non ancora eseguito** su Windows: navigazione, ricerca e integrazione del drop OS devono essere provate manualmente.
- Il divisore è trascinabile, ma non è ancora disponibile una modalità di pannelli a tre colonne.
- Nessuna drag & drop interno tra i pannelli, nessuna clipboard integrata nella Shell Windows; il drag & drop **in ingresso** è distinto da queste funzioni.
- La modalità Taglia/Sposta tra volumi differenti **non è supportata** (mostra errore; non cancella la sorgente). Usa Copia e verifica i contenuti prima di spostare gli originali nel Cestino.
- Operazioni batch **non transazionali**: una modifica concorrente durante un batch può lasciarne completata una parte. L'errore lo segnala esplicitamente.
- Nessuna progress bar o annullamento, anteprima, indicizzazione ricorsiva, menu contestuale Shell, undo, copia di link, ripresa copie, sincronizzazione con modifiche esterne.
- La copia preserva i contenuti, ma non garantisce conservazione di permessi ACL, metadati e timestamp. Evita al momento directory di sistema, file in uso o dati sensibili.

Consulta `SPECIFICHE.md` e `COLLAUDO_WINDOWS.md`.
