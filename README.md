# Veldir v0.8.0 — file manager leggero per Windows

Prototipo **open source MIT** in Rust + egui, ispirato alla filosofia di File Pilot, con codice e identità indipendenti. **Non affiliato a File Pilot.**

> **Stato reale:** fino alla v0.7.0 i controlli automatici Windows (check, test, build release, checksum) sono passati. La v0.8.0 deve superare gli stessi controlli. Il collaudo grafico e le operazioni con file reali restano da completare: provare solo su file di test.

## Novità v0.8.0 — Interfaccia più ordinata e PDF testuale

- **Barra superiore ridisegnata:** azioni più frequenti sempre visibili; menu «Operazioni» e «Visualizza» per ridurre la densità dei pulsanti.
- **Schede più compatte:** superfici evidenziate per la scheda attiva, chiusura integrata e percorsi mostrati al passaggio del mouse.
- **Intestazione e pannelli coordinati:** pannello attivo riconoscibile, navigazione dei percorsi preservata, barra di stato con dimensione totale dei file selezionati.
- **Prima ispezione PDF con F3:** numero di pagine e testo estratto (massimo due pagine e 8.000 caratteri), senza caricare interi documenti nell'interfaccia.
- **Limiti PDF:** massimo 12 MiB per documento, massimo 2.000 pagine, limite di decompressione per pagina. I PDF protetti, danneggiati e quelli acquisiti solo come immagine non vengono resi graficamente; per questi file usare il lettore PDF predefinito.
- **Nessun rendering grafico PDF:** questa versione mostra un *estratto testuale*, non una fotografia della pagina. Il rendering grafico resta un'attività futura separata.

## Novità v0.7.0 — Icone native e immagini

- **Icone native Windows Shell:** ogni tipo di file e le cartelle utilizzano le associazioni del sistema. Le icone sono recuperate su thread separati e memorizzate per estensione; se non sono disponibili viene usato il simbolo precedente.
- **Miniature immagini** nella lista per PNG, JPEG, WebP, GIF e BMP; decodifica soltanto per le righe visibili, con cache di massimo 128 immagini e al massimo sei richieste contemporanee.
- **Anteprima immagine** nella finestra F3, ridimensionata per rispettare l'area disponibile; mantiene l'anteprima testuale esistente.
- **Pulsante Miniature ON/OFF** con preferenza salvata tra le sessioni.
- **Limiti di sicurezza delle anteprime:** file regolari max 32 MiB, immagini max 30 megapixel, elaborazione in background e anteprima max 1200 px per lato. Non sono inclusi RAW, TIFF, PDF o filmati.
- **Nuovi test** su miniature, proporzioni, limite dimensione e chiave cache per tipo di file.

**Nota:** le icone Shell e il posizionamento delle miniature richiedono verifica GUI su Windows reale. Non è stato creato un visualizzatore PDF, né un sistema completo di gestione metadati.

## Novità v0.6.0 — Copia verso Windows e trascinamento interno

- **Copia Veldir → Esplora file:** Ctrl+C prepara i percorsi dei file nella clipboard interna e in quella di Windows (CF_HDROP), così Esplora file può incollarli. Il pulsante «Verso Esplora» esporta la selezione senza cambiare la clipboard interna.
- **Copia Esplora file → Veldir:** resta disponibile il pulsante «Da Esplora» o Ctrl+Maiusc+V, con conferma di destinazione.
- **Trascina tra pannelli:** trascina la riga di un file o una selezione multipla verso il pannello opposto. Il rilascio chiede conferma e **copia**, non sposta. Trascinare in altri punti non modifica file.
- **Taglia Ctrl+X resta interno a Veldir:** nessun effetto di spostamento viene pubblicato nella clipboard di Windows.
- **Sicurezza:** la clipboard viene aggiornata solo su richiesta esplicita; le sorgenti non vengono modificate dalla mera esportazione.

### Limitazioni della clipboard e del trascinamento

- CF_HDROP è per file realmente presenti sul filesystem: file virtuali in app come Outlook non sono gestiti. Limite 500 elementi esportati.
- Il trascinamento interno vale solo **tra due pannelli Veldir**, non verso Esplora file o altre finestre. Per trasferire verso Esplora utilizzare Ctrl+C e poi Incolla lì.
- L'esportazione pubblica l'intenzione di **copia**, non di taglio. Incollare con Ctrl+V *dentro Veldir* usa la clipboard interna; per importare quella Windows usare Ctrl+Maiusc+V.
- Nessun rollback automatico dei batch; link/junction e preservazione completa ACL/metadati non sono ancora supportati.

## Novità v0.5.0 — Trasferimenti più sicuri e clipboard Esplora

- **Avanzamento in byte** delle copie: mostra byte copiati/totali e velocità media in barra di stato, senza caricare l'intero file in RAM.
- **Annullamento durante una copia grande**: controllo a blocchi di 256 KiB, con eliminazione automatica dello staging parziale; nessun file incompleto viene pubblicato alla destinazione.
- **Copia di directory** con calcolo preventivo dei byte, protezione symlink/junction e pubblicazione senza sovrascrivere file esistenti.
- **Importa dalla clipboard di Esplora file (Windows)**: tasto «Da Esplora» oppure Ctrl+Maiusc+V, selezione destinazione e conferma.
- Import dalla clipboard **sempre in modalità COPIA**, anche se su Windows è stato usato Taglia: nessuna eliminazione accidentale delle sorgenti. Clipboard virtuale e formati diversi da CF_HDROP non sono supportati.
- Nuovi test automatici per integrità delle copie, progresso in byte e cancellazione a metà del file.

**Limitazioni:** le copie batch sono ancora non transazionali: gli elementi già completati prima dell'annullamento rimangono copiati. La velocità indicata è una media osservata, non una promessa di prestazioni. L'importazione clipboard non esporta file verso Esplora e non gestisce cut/move in modo bidirezionale. Per i trasferimenti tra dischi, lo spostamento automatico rimane disabilitato.

## Funzioni aggiunte nella v0.4.0

- Menu contestuale per file e cartelle (funzioni base Veldir, non ancora Shell Windows).
- Indicatori grafici per le principali estensioni, non ancora le icone native del sistema.
- Finestra ricerca ricorsiva (Ctrl+Maiusc+F), con corrispondenza fuzzy, massimo 250 risultati e arresto su richiesta; i collegamenti non vengono attraversati.
- Finestra dettagli e anteprima testuale (F3) per TXT, MD, JSON, XML, CSV, codice e log: massimo 32 KiB per file.
- Tema chiaro/scuro selezionabile, memorizzato.
- Rinomina multipla con anteprima: prefisso, sostituzione del testo nel nome e numerazione, con rifiuto di collisioni.
- Avanzamento per numero di elementi nei trasferimenti batch e annullamento tra un elemento e il successivo; il singolo file in corso non viene interrotto.
- Test per ricerca, anteprima, rinomina sicura e annullamento.

**Nota sicurezza:** in caso di cancellazione di un batch, gli elementi già completati rimangono modificati. L'app non ha rollback completo. La copia non preserva garantitamente ACL, timestamp e metadati. Evita file importanti fino al collaudo manuale della versione 0.4.

## Funzioni ancora mancanti

Rendering grafico PDF, thumbnail RAW/TIFF, clipboard Shell completa con taglio/spostamento, drag & drop verso applicazioni esterne, aggiornamento automatico cartelle, menu Shell, indicizzazione persistente, undo/rollback, strumenti ZIP completi, installer e firma digitale. Queste non devono essere considerate implementate.

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

Carica i file in **un repository Veldir dedicato**. In **Actions → Veldir Windows Build** viene compilata la versione Windows e, se tutti i controlli passano, viene pubblicato come artefatto `Veldir-v0.8.0-Windows-x64` con l'EXE e il suo SHA-256. La build v0.2.0 è stata verificata nella CI; **il collaudo manuale dell'interfaccia resta necessario**.

## Scorciatoie

| Comando | Tasti |
| --- | --- |
| Nuova scheda / chiudi scheda sinistra | Ctrl+T / Ctrl+W |
| Selezione aggiuntiva / intervallo / tutti i visibili | Ctrl+clic / Maiusc+clic / Ctrl+A |
| Passa tra i pannelli | F6 (apre il secondo se disattivato) |
| Modifica percorso / filtro pannello attivo | Ctrl+L / Ctrl+F |
| Indietro / avanti / cartella superiore | Alt+← / Alt+→ / Alt+↑ |
| Aggiorna entrambi i pannelli e le schede | F5 |
| Copia anche in Esplora / taglia interno / incolla interno | Ctrl+C / Ctrl+X / Ctrl+V |
| Importa da clipboard Esplora con conferma | Ctrl+Maiusc+V |
| Deseleziona / Rinomina uno / Cestino multiplo | Esc / F2 / Canc |
| Apri un elemento | Invio / doppio clic |

## Limitazioni ancora aperte

- **Non è una release stabile:** mancano il collaudo completo della GUI e test su PC diversi.
- Nessuna copia di link/junction, nessun rollback delle operazioni già completate e nessuna garanzia di preservare timestamp, ACL o altri metadati.
- Trasferimenti in modalità Sposta tra dischi diversi non supportati; la copia senza sovrascrittura è invece utilizzabile, seguita da verifica manuale.
- Clipboard Shell limitata a CF_HDROP con intenzione di copia; nessun drag & drop da Veldir verso applicazioni esterne.
- Le icone native e le miniature immagini sono state introdotte nella 0.7; restano assenti rendering grafico PDF, indicizzazione persistente, menu contestuale completo di Windows, file watcher, installer e firma digitale.


Consulta `SPECIFICHE.md` e `COLLAUDO_WINDOWS.md`.
