# Piano di collaudo Veldir 0.10.0-dev — Windows 10/11

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
| UI-05 | Apri 5-8 schede e passa tra loro | Etichette e tasti chiusura leggibili, scheda attiva riconoscibile |
| FILTER-03 | Attiva Documenti + 1–100 MiB + Ultimi 7 giorni | Solo file che soddisfano tutti i criteri, cartelle sempre visibili |
| FILTER-04 | Seleziona file, modifica un filtro | Selezione azzerata, nessuna operazione su elementi nascosti |
| FILTER-05 | Con filtri attivi premi Ctrl+A | Selezione solo degli elementi visibili |
| FILTER-06 | Premi × con più filtri attivi | Nome, tipo, dimensione e data resettati |
| WATCH-01 | Con Veldir aperto, crea un file in Esplora nella stessa cartella | Il file appare senza F5 dopo il debounce |
| WATCH-02 | Rinomina e poi elimina file da Esplora | L'elenco si aggiorna, nessuna selezione stale |
| WATCH-03 | Disattiva Aggiornamento automatico e modifica la cartella esternamente | Elenco stabile fino a F5; riattivando torna l'aggiornamento automatico |
| WATCH-04 | Prova su cartella UNC o disco rimovibile | Nessun crash se notifiche assenti o cartella scollegata |
| WATCH-05 | Copia un file grande nella cartella monitorata | UI responsiva, nessuna scansione ricorsiva o raffica di aggiornamenti |

| UI-06 | Apri menu Operazioni e Visualizza | Tutte le funzioni esistenti raggiungibili senza sovrapposizioni |
| UI-07 | Seleziona file di dimensioni diverse | Barra di stato mostra conteggio e dimensioni cumulativi |
| UI-08 | Ridimensiona la finestra da 900 px a schermo intero | Barra degli indirizzi, menu e pannelli non si sovrappongono |
| PDF-01 | Seleziona PDF testuale di una pagina e premi F3 | Numero pagine e contenuto iniziale, senza aprire il PDF |
| PDF-02 | Seleziona PDF scansione senza testo | Messaggio che l'anteprima testuale non è disponibile |
| PDF-03 | Seleziona PDF protetto o danneggiato | Errore non bloccante, possibilità di aprire col lettore esterno |
| PDF-04 | Seleziona PDF oltre 12 MiB | Rifiuto controllato senza bloccare la UI |
| PDF-05 | Seleziona PDF dopo foto e testo | Nessuna anteprima precedente rimane visibile |
| SORT-01 | Clicca Nome, Dimensione e Modificato due volte | Ordinamento crescente/decrescente, cartelle sempre prime |
| ICON-01 | Apri una cartella con PDF, JPG, DOCX, ZIP e cartelle | Icone associate ai tipi di Windows, senza blocchi |
| ICON-02 | Cambia directory e torna alla precedente | Icone caricate da cache, UI responsiva |
| IMG-01 | Attiva Miniature ON, apri cartella con JPG/PNG/WebP/GIF/BMP | Miniature visibili solo sulle righe caricate |
| IMG-02 | Apri una cartella con 1.000 immagini e scorri | Caricamenti non bloccanti; RAM e tempi da misurare |
| IMG-03 | Seleziona un JPEG e premi F3 | Anteprima a colori con proporzioni corrette |
| IMG-04 | Seleziona un'immagine >32 MiB o >30 MP | Rifiuto controllato, nessun blocco della UI |
| IMG-05 | Disattiva miniature, chiudi e riapri Veldir | Miniature rimangono disattivate; icone native visibili |
| IMG-06 | Cambia selezione rapidamente tra due immagini e testo | Non appare un'anteprima appartenente alla selezione precedente |
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
| CLIP-08 | Seleziona un file in Veldir, Ctrl+C, cambia pannello/cartella e Ctrl+V | Compare conferma trasferimento, file copiato e sorgente integra |
| CLIP-09 | Copia due file da Esplora con Ctrl+C e premi Ctrl+V in Veldir | Veldir riconosce CF_HDROP senza testo, richiede conferma e copia entrambi |
| CLIP-10 | Copia una cartella con Ctrl+C da Veldir e incolla con Ctrl+V nella stessa directory | Conflitto mostrato; Interrompi non sovrascrive, Conserva entrambi crea nome distinto |
| CLIP-11 | Con filtro percorso attivo digita testo, Ctrl+A, Ctrl+C e Ctrl+V | Opera sul testo del filtro, non sulla selezione dei file |
| CLIP-12 | Copia file in Veldir, poi copia testo semplice in un'altra applicazione e Ctrl+V nel pannello | Nessun file vecchio incollato per errore; avviso clipboard senza file |
| CLIP-13 | Ctrl+X in Veldir, Ctrl+V in un'altra cartella Veldir | Conferma dello spostamento, nessuna eliminazione prima della conferma |
| CLIP-14 | Ctrl+X su file A in Veldir, poi Ctrl+C su file B in Esplora e Ctrl+V in Veldir | Si COPIA B dopo conferma; A non si sposta e rimane intatto |
| CLIP-15 | Ctrl+X su file A in Veldir, poi copia testo in Blocco note e Ctrl+V sul pannello | Nessun file viene spostato; compare avviso di assenza file negli appunti |
| CLIP-16 | Ctrl+C su file in Esplora, apri Veldir senza selezionare file e clicca «Incolla» | Pulsante abilitato, finestra di conferma, sorgente intatta |
| CLIP-17 | Con una copia di file in Windows premi e rilascia il solo tasto V, senza Ctrl | Nessun trasferimento viene avviato |
| CLIP-18 | Ctrl+C in Veldir e poi copia testo in un'altra applicazione prima di Ctrl+V in Veldir | Non vengono incollati file obsoleti |
| CLIP-19 | Seleziona più file con spazi, accenti e caratteri Unicode in Veldir; premi Ctrl+C e incolla in Esplora | Tutti i file corretti, nomi integri e sorgenti conservate |
| CLIP-20 | Copia due file con nome Unicode da Esplora e incollali in Veldir | Entrambi i percorsi CF_HDROP letti correttamente, conferma prima della copia |
| CLIP-21 | Mentre un'altra finestra è in primo piano, verifica che nessuna funzione automatica esporti appunti come appartenenti a un'altra applicazione | Nessuna sostituzione indesiderata della clipboard di sistema |

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

## Verifiche spazio disponibile delle unità (nuova implementazione)
| ID | Procedura | Risultato atteso |
| --- | --- | --- |
| DRIVE-01 | Apri Veldir con C:\\ e un secondo disco locale | Ogni voce mostra `... GB liberi` senza bloccare la GUI |
| DRIVE-02 | Confronta i valori con Proprietà disco di Esplora risorse | Quantità libera sostanzialmente coerente, tenendo conto di arrotondamenti e quote |
| DRIVE-03 | Verifica unità di rete mappate X:\\, Y:\\, Z:\\ accessibili | Valore disponibile anche per il mapping di rete, senza attendere il completamento delle altre unità |
| DRIVE-04 | Rendi temporaneamente irraggiungibile un'unità di rete di prova e riapri | La UI resta reattiva; `n/d` in caso di errore, non un valore numerico fittizio |
| DRIVE-05 | Copia file di prova su un volume, poi premi F5 | Spazio disponibile aggiornato, senza richiedere riavvio |
| DRIVE-06 | Apri molti preferiti e varie unità | Barra laterale scorrevole, le voci non vengono tagliate |
| DRIVE-07 | Controlla il tooltip di un volume | Spazio libero disponibile all'utente e capacità totale coerenti |

## Verifiche dimensione cartelle (nuova implementazione)

| ID | Procedura | Risultato atteso |
| --- | --- | --- |
| SIZE-01 | Crea cartella vuota e cartella con file da 10 KiB + sottocartella con 20 KiB | 0 B e 30 KiB; mai soltanto la dimensione del contenitore |
| SIZE-02 | Apri cartella con molte sottocartelle e scorri | Calcolo in background, interfaccia fluida e massimo 2 calcoli simultanei |
| SIZE-03 | Ordina per Dimensione; attendi il completamento | Ordinamento delle cartelle aggiornato in base ai valori calcolati |
| SIZE-04 | Modifica contenuto di una sottocartella e premi F5 | Risultati non rimangono obsoleti |
| SIZE-05 | Visualizza → disattiva e riattiva Calcola dimensioni cartelle, poi riavvia | Preferenza persistente, «—» quando disattivato |
| SIZE-06 | Prova directory con symlink/junction e percorso senza accesso | «n/d» e tooltip esplicativo, senza seguire collegamenti o inventare totali |
| SIZE-07 | Prova condivisione remota lenta e cartella enorme | Veldir resta utilizzabile, dimensione non disponibile quando supera i limiti |

## Note
Le copie batch possono fermarsi dopo alcuni elementi già completati, non esiste rollback automatico. Copie non garantiscono preservazione metadati e ACL. Testare ogni workflow e controllare l'EXE ottenuto dalla CI prima di usare il programma su file reali.


## Restyling dopo v0.9.0 — collaudo GUI da eseguire
- [ ] Avvio con Windows in tema chiaro e preferenza Veldir scura: nessun testo scuro o pulsante bianco.
- [ ] Alternare tema chiaro/scuro, riavviare e verificare persistenza, menu, dialoghi, ricerca e anteprima.
- [ ] Verificare contrasto di righe alternate, hover, selezione singola/multipla e finestra inattiva.
- [ ] A 900, 1230 e 1920 pixel, DPI 100/125/150/200%, verificare barra a tutta larghezza e comandi accessibili.
- [ ] Nomi lunghi troncati, tooltip completo, colonne allineate; due pannelli e divisore ai limiti.
- [ ] Icone Shell nitide e trasparenti; miniature con proporzioni corrette, fallback senza simboli mancanti.
- [ ] Doppio clic, selezione Ctrl/Maiusc, menu contestuale e trascinamento fra pannelli dopo il nuovo hit target di riga.
La CI verifica compilazione e test; queste verifiche visive e di interazione richiedono una sessione Windows reale.


## Verifiche aggiunte per i due pannelli
- Rendering automatico dell'interfaccia completa a 900/1230/1920 punti, tema chiaro e scuro, divisore 25/50/75%: nessuna sovrapposizione o crescita oltre il viewport.
- Input sintetico F6 e Ctrl+A: cambio pannello e selezione dei soli file filtrati nel pannello attivo.
- Correzione: rettangoli dei pannelli riservati prima del rendering, clipping separato e campo filtro adattivo.
- Tentativo GUI sul computer locale: la finestra viene individuata, ma la cattura restituisce "FrameArrived timed out" e, dopo recupero, "window capture timed out". Nessuna interazione GUI certificata da questo tentativo.
- Da completare manualmente: trascinamento del divisore, clic sulle righe, Ctrl/Maiusc, menu contestuale, doppio clic, trascinamento fra pannelli e DPI 125/150/200%.
