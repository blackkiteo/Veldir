# Piano di collaudo Veldir 0.6.0 — Windows 10/11

**Eseguire solo su file di prova, mai su dati importanti.** Il passaggio della CI non sostituisce un collaudo manuale della GUI e delle operazioni.

## Preparazione
Crea due cartelle `C:\Veldir_Test\Origine` e `C:\Veldir_Test\Destinazione`, popolando la prima con file di testo, immagini e sottocartelle di prova.

## Test manuali
| ID | Procedura | Risultato atteso |
| --- | --- | --- |
| BUILD-01 | `cargo check`, `cargo test`, `cargo build --release` | Nessun errore ed EXE presente |
| NAV-01 | Doppio clic su cartelle, Alt+frecce, Ctrl+L | Navigazione coerente, nessun crash |
| FOCUS-01 | Ctrl+L, digitare un percorso valido e premere Invio | Il campo indirizzo riceve il focus e la navigazione avviene correttamente |
| FOCUS-02 | Ctrl+F, digitare un nome | Il campo filtro del pannello attivo riceve il focus |
| FILTER-01 | Selezionare più file, cambiare filtro, premere Canc | Le selezioni vengono azzerate: nessun elemento nascosto viene eliminato |
| FILTER-02 | Selezionare file, azzerare filtro con × | Nessuna selezione residua |
| MODAL-01 | Aprire conferma Cestino, premere Ctrl+W o Ctrl+T | La finestra rimane coerente e nessuna scheda viene chiusa/aperta |
| PAN-01 | Attiva due pannelli e apri cartelle diverse | Navigazioni indipendenti |
| UI-01 | Ridimensiona i pannelli trascinando il divisore | Larghezze cambiano senza blocchi o elementi sovrapposti |
| UI-02 | Chiudi e riapri Veldir dopo aver regolato il divisore | Le proporzioni sono ripristinate |
| UI-03 | Nascondi la barra laterale e riapri | Stato della barra conservato |
| UI-04 | Clicca su una cartella nella barra percorso | Navigazione immediata alla cartella scelta |
| SORT-01 | Clicca Nome, Dimensione e Modificato due volte | Ordinamento crescente/decrescente, cartelle sempre prime |
| SEL-03 | Dopo aver filtrato, usa Ctrl+A e poi Esc | Selezionati solo file visibili; Esc azzera |
| PAN-02 | Cambia pannello con clic e F6 | Toolbar applicata al pannello attivo |
| PAN-03 | Chiudi e riapri l'app in modalità doppia | Percorsi ripristinati |
| SEL-01 | Ctrl+clic e Maiusc+clic | Selezione multipla corretta |
| SEL-02 | Ripeti selezione su elenco filtrato | Nessun crash |
| BATCH-01 | Copia due file tra pannelli | Entrambi copiati, sorgenti conservate |
| COPY-01 | Copia file da 2 GB e premi Annulla durante il trasferimento | Sorgente integra, nessun file parziale di destinazione, nessuna cartella staging residua |
| COPY-02 | Copia cartella con più file e sotto-cartelle | La barra mostra byte/byte e file completati, senza bloccarsi |
| CLIP-01 | Copia 2 file con Ctrl+C in Esplora, in Veldir usa «Da Esplora» | Finestra di conferma, entrambi copiati, originali intatti |
| CLIP-02 | Taglia un file in Esplora, importalo in Veldir | Solo copia: file originale ancora presente |
| CLIP-03 | Clipboard contenente testo, premi «Da Esplora» | Messaggio di errore, nessuna modifica ai file |
| CLIP-04 | Seleziona due file in Veldir, Ctrl+C, poi Ctrl+V in Esplora | I file sono copiati nella cartella Windows; originali intatti |
| CLIP-05 | Seleziona una cartella in Veldir, «Verso Esplora», Incolla in Esplora | Copia della cartella con conferma Windows standard |
| CLIP-06 | Usa Ctrl+X in Veldir, poi incolla in Esplora | Nessun taglio della sorgente: il taglio rimane interno a Veldir |
| CLIP-07 | Chiudi e riapri Veldir dopo Ctrl+C, poi incolla in Esplora | La clipboard Windows dei percorsi rimane valida fino alla sostituzione del contenuto |
| DND-04 | Trascina da pannello sinistro al destro (cartelle di test) | Richiesta conferma e sola copia, originali intatti |
| DND-05 | Seleziona più file, trascina uno dei selezionati al pannello opposto | Copia di tutti i file selezionati con conferma |
| DND-06 | Rilascia un file sul pannello di origine o fuori dai pannelli | Nessuna copia o spostamento |
| DND-07 | Rilascia su pannello opposto e premi Annulla | Nessuna modifica |

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
