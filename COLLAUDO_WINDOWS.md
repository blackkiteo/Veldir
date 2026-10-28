# Piano di collaudo Veldir 0.2 — Windows 10/11

**Eseguire solo su file di prova, mai su dati importanti.** Il passaggio della CI non sostituisce un collaudo manuale della GUI e delle operazioni.

## Preparazione
Crea due cartelle `C:\Veldir_Test\Origine` e `C:\Veldir_Test\Destinazione`, popolando la prima con file di testo, immagini e sottocartelle di prova.

## Test manuali
| ID | Procedura | Risultato atteso |
| --- | --- | --- |
| BUILD-01 | `cargo check`, `cargo test`, `cargo build --release` | Nessun errore ed EXE presente |
| NAV-01 | Doppio clic su cartelle, Alt+frecce, Ctrl+L | Navigazione coerente, nessun crash |
| PAN-01 | Attiva due pannelli e apri cartelle diverse | Navigazioni indipendenti |
| PAN-02 | Cambia pannello con clic e F6 | Toolbar applicata al pannello attivo |
| PAN-03 | Chiudi e riapri l'app in modalità doppia | Percorsi ripristinati |
| SEL-01 | Ctrl+clic e Maiusc+clic | Selezione multipla corretta |
| SEL-02 | Ripeti selezione su elenco filtrato | Nessun crash |
| BATCH-01 | Copia due file tra pannelli | Entrambi copiati, sorgenti conservate |
| BATCH-02 | Copia con nome già esistente | Nessuna sovrascrittura; preflight blocca |
| BATCH-03 | Annulla spostamento confermato | Nessuna modifica |
| BATCH-04 | Sposta file nello stesso volume | Destinazione aggiornata |
| BATCH-05 | Annulla richiesta di Cestino | Nessuna modifica |
| BATCH-06 | Conferma richiesta di Cestino | File nel Cestino quando supportato |
| DND-01 | Trascina file da Esplora file | Richiesta conferma e destinazione |
| DND-02 | Annulla il drop | Nessuna modifica |
| DND-03 | Conferma drop | Copie presenti, sorgenti intatte |
| FS-01 | Copia cartella in una sua sottocartella | Rifiuto sicuro |
| FS-02 | Copia cartella con junction/symlink | Rifiuto senza pubblicazione parziale |
| PERF-01 | Apri cartella di 5.000 file | UI reattiva, misurare tempi |
| PERF-02 | Registra RAM/CPU a riposo | Numeri misurati, niente stime |

## Note
Le copie batch possono fermarsi dopo alcuni elementi già completati, non esiste rollback automatico. Copie non garantiscono preservazione metadati e ACL. Testare ogni workflow e controllare l'EXE ottenuto dalla CI prima di usare il programma su file reali.
