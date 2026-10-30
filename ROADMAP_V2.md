# Veldir — Roadmap V2 completa
**Piano di sviluppo per Windows 10/11 x64, dalla 0.10.0-dev alla 2.0**

Aggiornata: 30 ottobre 2026 · Stato: proposta operativa, non promessa di release · Licenza: MIT.

> Questa è una nuova roadmap di prodotto, distinta da `ROADMAP.md` (cronologia dello sviluppo) e dalla [issue #5](https://github.com/blackkiteo/Veldir/issues/5) (le 50 attività storiche). Non trasformare una casella in "completata" soltanto perché del codice esiste. I controlli di codice, GUI e release sono separati.

## 1. Visione e ambito

**Veldir** deve diventare un file manager Windows nativo nell'esperienza d'uso, veloce, leggero e orientato a tastiera e mouse: doppio pannello, schede, copia/spostamento affidabili, ricerca efficace, anteprime e pieno controllo sulle operazioni. Ispirazione funzionale a File Pilot, senza copiarne branding o interfacce protette.

Principi non negoziabili:
1. **Integrità dei dati prima di ogni funzione:** niente sovrascritture o eliminazioni implicite; le operazioni a metà non vengono dichiarate riuscite.
2. **Reattività:** elenchi, miniature, ricerca, hashing e copie non devono bloccare il thread UI.
3. **Comportamento Windows prevedibile:** dialoghi, Explorer, tastiera, DPI, accessibilità e dischi di rete/rimovibili.
4. **Portabilità e privacy:** applicazione Rust/eframe senza Electron, account, telemetria o servizi cloud obbligatori.
5. **Funzioni piccole e verificabili:** ogni milestone è una PR o serie limitata di PR, con test e criteri d'uscita.
6. **Prima la qualità della 1.0, poi le funzioni avanzate:** 1.0 non equivale a tutte le funzioni di Esplora.

Destinazione primaria: Windows 11 x64; compatibilità Windows 10 x64 da collaudare. Distribuzione iniziale: ZIP portabile. Eventuale installer solo dopo, se giustificato.

### Legenda
- **Fatto nel codice**: già implementato, da non riscrivere inutilmente.
- **Da collaudare**: presente nel codice o nei test automatici, ma senza prova manuale sufficiente.
- **Da sviluppare**: non completo o non presente.
- **P0**: blocca la 1.0; **P1**: alto valore ma non deve bloccarla se documentato; **P2**: miglioramento futuro.
- **DoD** (Definition of Done): codice + test + prova realistica pertinente + documentazione + CI verde; per feature critiche, verifica dei contenuti prima/dopo.

## 2. Baseline verificata al 30 ottobre 2026

| Area | Stato reale |
|---|---|
| Codice e release | `0.10.0-dev` sul ramo `feat/roadmap-to-1-0`; PR [#12](https://github.com/blackkiteo/Veldir/pull/12) **draft e non unita a main** |
| CI Windows MSVC | Ultimo build verificato [run 41001059892](https://github.com/blackkiteo/Veldir/actions/runs/41001059892): **68 test superati, zero falliti, un benchmark separato superato**, compilazione release, ZIP e checksum |
| Interfaccia | Doppio pannello, schede sul pannello sinistro, temi, divisore, sidebar, breadcrumb, preferiti, ordinamento e filtri combinabili: implementati |
| Tastiera/clipboard | Navigazione e `Ctrl+C`/`Ctrl+V`/`Ctrl+X`; CF_HDROP Windows, aggiornamenti generazione clipboard, controlli batch e nomi Unicode: implementati e testati nel backend; interazione GUI con Explorer da collaudare |
| File | Copia con staging e annullamento, spostamento stesso volume, Cestino, rinomina multipla e conflitti Stop/Skip/KeepBoth: implementati, da collaudare su filesystem reali |
| Ricerca/anteprime | Ricerca ricorsiva, fuzzy, filtri, watcher, icone Shell, miniature immagini, anteprima testo/UTF-16, PDF **solo testuale** |
| Distribuzione | ZIP portabile e opzione `--portable`, checksum, metadati build; firma digitale assente |
| Gate aperti | [#13](https://github.com/blackkiteo/Veldir/issues/13) GUI/release, [#15](https://github.com/blackkiteo/Veldir/issues/15) protezioni di `main` |

**Limiti attuali importanti:** spostamento automatico tra volumi non supportato; nessun rollback universale o preservazione garantita di ACL e metadati; niente Shell context menu completo, PDF grafico, clipboard MOVE bidirezionale o drag verso applicazioni esterne. Le prestazioni GUI e la RAM reale non sono ancora certificate.

## 3. Piano esecutivo fino alla 1.0

### M0 — 0.10.x: chiudere i problemi critici esistenti (P0)

**Obiettivo:** portare le funzioni già presenti a un livello di affidabilità dimostrabile. Questa fase viene prima di introdurre nuove funzioni.

- [ ] **V2-001** Collaudo Windows di copia/incolla Veldir↔Veldir, Veldir↔Explorer e di `Ctrl+X` con clipboard sostituita; testo nei campi, popup e focus.
- [ ] **V2-002** Verificare su GUI reale file Unicode, 500 elementi, file scomparsi, formati solo testuali e app con clipboard virtuale; errore esplicito e nessuna copia parziale silenziosa.
- [ ] **V2-003** Eseguire i casi `COLLAUDO_WINDOWS.md` per drag & drop interno, selezioni multiple, filtri, rename e conferme.
- [ ] **V2-004** Verificare operazioni su file grandi, cartelle annidate, file bloccati, cartella protetta, disco pieno e perdita di accesso alla destinazione.
- [ ] **V2-005** Allineare README, SPECIFICHE e checklist: eliminare riferimenti obsoleti alle funzioni già introdotte (per esempio il watcher).
- [ ] **V2-006** Audit del recupero preferenze, migrazione della versione dei dati e ripristino schede/percorso non disponibile.
- [ ] **V2-007** Preparare una cartella fixtures di test innocue con nomi Unicode, strutture annidate e file di riferimento per confronto byte/hash.
- [ ] **V2-008** Attivare ruleset di protezione `main` e check CI obbligatori: azione manuale del proprietario, issue #15.
- [ ] **V2-009** Registrare bug riproducibili con versione, Windows, DPI, passaggi e risultato atteso; correggere P0 prima di proseguire.

**Uscita M0:** nessun difetto noto di perdita/sovrascrittura non richiesta; scenari clipboard e GUI P0 verificati; issue #13 aggiornata con evidenze. I test automatici da soli non chiudono questa fase.

### M1 — 0.11: motore trasferimenti affidabile (P0)

**Obiettivo:** operazioni prevedibili anche su volumi diversi, errori e batch complessi.

- [ ] **V2-010** Separare preflight, pianificazione, esecuzione, progresso e risultati del motore trasferimenti in moduli testabili; ridurre accoppiamenti con `src/app.rs`.
- [ ] **V2-011** Implementare spostamento **fra volumi** con copia in staging, verifica di integrità, pubblicazione senza sostituzione e **rimozione della sorgente solo dopo successo confermato**; gestione conservativa di fallimenti ed errori di cleanup.
- [ ] **V2-012** Esporre una finestra di riepilogo che distingua copiati, spostati, saltati, non riusciti e rimasti in origine, compresi esiti parziali.
- [ ] **V2-013** Consolidare politiche conflitto: Interrompi, Salta, Conserva entrambi; stessa logica per file e directory, anche con omonimi in un batch.
- [ ] **V2-014** Proteggere copia su discendente, loop, link simbolici/junction/reparse point, cartelle non accessibili e percorsi che cambiano durante il job.
- [ ] **V2-015** Annullamento affidabile durante file e directory grandi: staging rimosso quando sicuro, elementi già pubblicati esplicitamente elencati.
- [ ] **V2-016** Definire e testare **quali metadati** vengono mantenuti (almeno timestamp di modifica, se fattibile) e quali no (ACL, ADS, attributi speciali), senza dichiarare equivalenza non verificata.
- [ ] **V2-017** Evitare blocchi UI e race tra watcher e operazioni file; invalidazione cache, refresh coerente e cancellazione delle selezioni obsolete.
- [ ] **V2-018** Suite di regressione su SSD locale, seconda unità, SMB e rimovibile con verifica contenuti, byte e hash sui casi critici.

**Uscita M1:** spostamento tra dischi effettuato senza cancellazioni premature; errori parziali sempre dichiarati; nessuna sovrascrittura silenziosa. Se non soddisfa questi criteri, la funzionalità rimane disabilitata e non è presentata come supportata.

### M2 — 0.12: esperienza d'uso, tastiera e accessibilità (P0/P1)

**Obiettivo:** utilizzo quotidiano intuitivo, coerente e completo senza mouse.

- [ ] **V2-019** Verificare focus, cambio pannello, selezione Ctrl/Maiusc, frecce, Home/Fine, PageUp/Down, typeahead, F2/F3/F5/F6 e popup senza scorciatoie accidentali.
- [ ] **V2-020** Rifinire righe, icone native di file/cartelle/unità/collegamenti, troncamento nomi, colonne, spaziature e tooltip.
- [ ] **V2-021** Uniformare azioni delle toolbar, menu contestuale base e scorciatoie: un comando deve avere lo stesso significato in ogni punto.
- [ ] **V2-022** Migliorare stati vuoto/caricamento/errore; messaggi italiani chiari e azione Riprova utile.
- [ ] **V2-023** Verificare ridimensionamento, doppio pannello, affiancamento Windows, multi-monitor e DPI 100/125/150/200%.
- [ ] **V2-024** Accessibilità con NVDA e UI Automation: righe annunciate, focus leggibile, controlli etichettati, navigazione logica e contrasto.
- [ ] **V2-025** Verificare persistenza di schede, colonne, layout e preferenze dopo chiusura inattesa e aggiornamento.
- [ ] **V2-026** Aggiornare guida scorciatoie, menu Informazioni e feedback delle azioni distruttive.
- [ ] **V2-027 (P1)** Schede anche nel pannello destro e preferiti drag-and-drop solo dopo aver stabilizzato selezione/focus; non blocca la 1.0.

**Uscita M2:** workflow principali eseguibili con tastiera; nessun comando file si attiva mentre l'utente digita in un campo testo; UI leggibile alle combinazioni DPI/tema testate.

### M3 — 0.13: ricerca e anteprime solide (P0/P1)

**Obiettivo:** migliorare ciò che esiste senza ampliare inutilmente la superficie di rischio.

- [ ] **V2-028** Testare ricerca ricorsiva e fuzzy su directory molto grandi, con risultati paginati/limitati, cancellazione e directory lente.
- [ ] **V2-029** Gestire file rimossi o rinominati durante la ricerca; la selezione e "Apri posizione" devono fallire in modo recuperabile.
- [ ] **V2-030** Ridurre blocchi e picchi di memoria di icone, miniature e anteprime con budget e limite alle richieste concorrenti.
- [ ] **V2-031** Verificare anteprima testo UTF-8/UTF-16, file binari, immagini grandi/corrotte e PDF testuali/scansionati con fallback esterno.
- [ ] **V2-032** Opzioni di visualizzazione e ordinamento persistenti per pannello; filtri sempre non distruttivi.
- [ ] **V2-033** Verificare watcher quando file cambiano esternamente e su cartelle condivise (fallback F5 se le notifiche mancano).
- [ ] **V2-034 (P1)** Valutare miniature TIFF/RAW e vista griglia; implementarle soltanto se non rallentano il percorso principale.
- [ ] **V2-035 (P1)** Valutare renderer PDF con isolamento, limiti di risorse, licenza e manutenzione. **Non obbligatorio per la 1.0**: il PDF attuale resta testuale.

**Uscita M3:** nessun crash o freeze noto per input anteprima malformato entro i limiti supportati; ricerca annullabile e risultati coerenti.

### M4 — 0.14: prestazioni, sicurezza e manutenzione (P0)

**Obiettivo:** arrivare a una candidata 1.0 misurabile e sostenibile.

- [ ] **V2-036** Profilare avvio a freddo/caldo, CPU/RAM a riposo, apertura 1.000/10.000/100.000 file, ricerca, miniature e scorrimento; distinguere backend da tempo percepito GUI.
- [ ] **V2-037** Registrare mediana e P95 su hardware identificato; usare soglie concordate **prima dei test** e non inventare risultati.
- [ ] **V2-038** Catturare regressioni di memoria/GDI handles, cancellazione di job, watcher, texture, icone e cache.
- [ ] **V2-039** Ampliare test di error handling, percorsi lunghi, Unicode, permessi, UNC, reparse point, rete lenta e unità staccate.
- [ ] **V2-040** Audit delle chiamate Win32/COM unsafe, ownership HGLOBAL/HICON, clipboard e thread affinity.
- [ ] **V2-041** Refactoring incrementale del grande `src/app.rs` in moduli per stato UI, comandi, sessioni, clipboard, trasferimenti e viste senza cambiare comportamento.
- [ ] **V2-042** Revisione dipendenze, licenze, avvisi di sicurezza e script CI; continuità GitHub Actions.
- [ ] **V2-043** Aggiungere o consolidare test smoke GUI automatizzabili senza confonderli con il collaudo umano.
- [ ] **V2-044** Confronto prestazionale ripetibile con Esplora e un file manager di riferimento a parità di dati, cache e hardware: risultati riportati senza marketing non verificato.

**Uscita M4:** budget prestazionali documentati, nessuna regressione critica, codice verificabile e CI ripetibile.

### M5 — 1.0 RC1 → RC2: collaudo finale e blocco modifiche (P0)

- [ ] **V2-045** Congelare le nuove funzioni e pubblicare internamente un candidato RC con commit, versione e checksum univoci.
- [ ] **V2-046** Eseguire matrice reale su Windows 11 e Windows 10, idealmente su due PC, dischi locali, USB, SMB, due monitor e DPI multipli.
- [ ] **V2-047** Confermare operazioni principali su cartelle **di prova** con confronto byte/hash: copia, spostamento stesso/altro volume, rinomina, Cestino, annullamento e collisioni.
- [ ] **V2-048** Verificare clipboard bidirezionale **copy-only** con Explorer, focus campi testo, selezioni multiple e file scomparsi; non promettere MOVE Shell completo.
- [ ] **V2-049** Verificare startup, installer assente/ZIP portabile, impostazioni standard/`--portable`, aggiornamento manuale e ripristino preferenze.
- [ ] **V2-050** Classificare bug in blocca-release/alta/media/bassa; zero bug bloccanti noti come condizione necessaria.
- [ ] **V2-051** Decidere **firma digitale**: firmare con certificato legittimo verificato e rigenerare checksum, oppure dichiarare esplicitamente pacchetto non firmato e i suoi avvisi SmartScreen.
- [ ] **V2-052** Chiudere documentatamente i gate di `RELEASE_CHECKLIST.md` e l'issue #13, con prove; non marcarli completati in anticipo.
- [ ] **V2-053** Verificare che il ruleset di `main` (issue #15) sia attivo e che i check critici siano richiesti.

**Uscita M5:** build RC identica a quella realmente collaudata; nessun difetto bloccante; documentazione e comportamento coincidono.

### M6 — Veldir 1.0 stabile: pubblicazione (P0)

- [ ] **V2-054** Review della PR #12 e delle PR successive, merge controllato su `main` solo dopo gate superati.
- [ ] **V2-055** Tag `v1.0.0` dal commit approvato, CI finale sul tag, checksum del binario esatto, pacchetto ZIP x64.
- [ ] **V2-056** GitHub Release con changelog, requisiti, funzioni, limiti dichiarati, installazione e modalità portabile.
- [ ] **V2-057** Istruzioni aggiornamento da 0.9/0.10 e test di compatibilità della configurazione.
- [ ] **V2-058** Verificare download ZIP, checksum, firma se presente e funzionamento EXE estratto su PC pulito.
- [ ] **V2-059** Documentare politica bugfix, sicurezza e supporto delle versioni 1.0.x.
- [ ] **V2-060** Aprire le attività residue in issue assegnate alla 1.1+, senza dichiararle già implementate.

**La 1.0 è un prodotto stabile e dichiaratamente delimitato**, non una promessa di tutte le estensioni di Esplora: il PDF grafico, i menu Shell completi, indicizzazione persistente, rollback totale e formati clipboard virtuali restano fuori scope salvo decisione esplicita.

## 4. Roadmap dopo la 1.0

### Veldir 1.1 — integrazione profonda con Windows (P1)
- [ ] **V2-061** Menu contestuale Shell completo, con handler di terze parti isolati/controllati e test COM.
- [ ] **V2-062** Apri con, Proprietà e dialoghi Windows nativi; icone e stati LNK/drive/junction coerenti.
- [ ] **V2-063** Drag & drop **in uscita** verso Explorer/applicazioni supportate; trasferimento del reale intento Copy/Move solo se sicuro.
- [ ] **V2-064** Clipboard MOVE bidirezionale e formati virtuali: valutare compatibilità e rischio di cancellazioni premature, con fallback conservativo.
- [ ] **V2-065** Miglior supporto UNC/SMB/OneDrive e notifiche nei filesystem virtuali; integrazione Associa cartella solo opzionale.
- [ ] **V2-066** Installer MSIX/MSI opzionale e associazioni cartelle, mantenendo ZIP portabile.

### Veldir 1.2 — produttività e strumenti (P1)
- [ ] **V2-067** Palette comandi con ricerca azioni e scorciatoie configurabili, verifica dei conflitti.
- [ ] **V2-068** Schede simmetriche, gruppi e workspace salvati, preferiti organizzabili, cronologia avanzata.
- [ ] **V2-069** Cronologia **reversibile** e Undo per subset sicuro (rinomina/move/copia) con log, vincoli espliciti, mai rollback fittizio.
- [ ] **V2-070** Coda trasferimenti, pausa/ripresa dove tecnicamente sostenibile, priorità e riepilogo persistente.
- [ ] **V2-071** Navigazione e operazioni sugli archivi ZIP, estrazione sicura con prevenzione Zip Slip, nessuna esecuzione automatica.
- [ ] **V2-072** Rinomina multipla evoluta con regex, anteprima conflitti e salvataggio preset; funzioni senza perdita di dati.
- [ ] **V2-073** Confronto cartelle e selezione differenze in sola lettura prima di introdurre qualsiasi sincronizzazione bidirezionale.

### Veldir 1.3 — visualizzazione e ricerca evoluta (P1/P2)
- [ ] **V2-074** Vista griglia, miniature più ricche e proprietà file avanzate con caricamento differito.
- [ ] **V2-075** Rendering grafico PDF controllato, pagina singola/zoom; esame licenza e sicurezza del motore prima dell'integrazione.
- [ ] **V2-076** Anteprime TIFF/RAW/audio/video e metadati EXIF quando disponibili, con limiti CPU/memoria e fallback esterno.
- [ ] **V2-077** Indicizzazione locale **opzionale**, incrementale, escludibile e cancellabile; nessuna scansione nascosta permanente.
- [ ] **V2-078** Ricerca contenuto nei documenti supportati, filtri salvati e risultati progressivi.
- [ ] **V2-079** Analisi duplicati tramite hash on-demand e verifica prima di proporre qualsiasi eliminazione.

### Veldir 2.0 — architettura ed estensibilità (P2, solo dopo validazione 1.x)
- [ ] **V2-080** Consolidare separazione core filesystem, services, Windows adapter e UI con API interne stabili e test indipendenti.
- [ ] **V2-081** Valutare estensioni/plugin sandbox e permessi espliciti, senza eseguire codice non fidato come funzione predefinita.
- [ ] **V2-082** Valutare più finestre e workspace, sessioni sincronizzabili **solo se esplicitamente attivate**.
- [ ] **V2-083** Valutare connessioni remote SFTP/FTP solo dopo threat model, gestione credenziali e trasferimenti sicuri.
- [ ] **V2-084** Aggiornamento applicazione verificato con firma/hash e rollback dell'aggiornamento (non dei file), sempre disattivabile.
- [ ] **V2-085** Design system e localizzazione completa italiano/inglese, guide e documentazione per contributori.

## 5. Criteri misurabili e matrice collaudo

**Ogni milestone deve avere evidenze**, non soltanto checkbox:
- **Correttezza dati:** confrontare sorgente/destinazione con byte/hash per test critici; verificare assenza di sovrascritture non confermate e file parziali pubblicati.
- **Performance:** startup a freddo e caldo; tempo prima che compaia il primo elenco; fluidità di scroll; RAM e CPU idle; costi miniature e ricerca. Misurare almeno 3 ripetizioni e riportare hardware/OS/numero file.
- **Scenari filesystem:** SSD, disco esterno, cartelle senza permessi, SMB, file con nomi Unicode e percorsi lunghi, link/junction, file bloccati e disconnessioni.
- **GUI:** Windows 10/11, tema scuro/chiaro, DPI 100/125/150/200%, schermo singolo/doppio, uso soltanto tastiera, NVDA.
- **Sicurezza:** input arbitrari, clipboard malformata, nomi conflittuali, ZIP traversal (se introdotto), limiti decompressione PDF e immagini.
- **Distribuzione:** checksum verificato, licenza, versione e commit build, test EXE portabile fuori dalla cartella sorgente, decisione firma e documentazione SmartScreen.

**Budget prestazionali:** fissare i valori numerici *dopo* il primo profilo su PC di riferimento e prima di ottimizzare; i tempi backend da 1k/10k file non sono tempi GUI. Nessun risultato deve essere inventato.

## 6. Decisioni e rischi

| Tema | Decisione raccomandata |
|---|---|
| Scope della 1.0 | Affidabilità del file manager di base prima di PDF grafico, ZIP e Shell menu esteso |
| Spostamento tra volumi | Da sviluppare con copia + verifica + rimozione sorgente solo dopo successo, non un semplice rename cross-device |
| Gestione sostituzione file | Default senza overwrite. Modalità "Sostituisci" soltanto con backup e recuperabilità dimostrata, altrimenti 1.1+ |
| Annullamento | Non promettere rollback dei batch già completati; comunicare esattamente ciò che è stato modificato |
| Shell clipboard | Fino a 1.0 intenzione Copy supportata; non promettere Cut/Move nativo fra app |
| PDF/archivi | Parser soggetti a limiti, file non fidati, manutenzione e licenze; separare dai gate P0 |
| Release | ZIP portabile come canale principale; firma solo se disponibile certificato valido o esplicita pubblicazione unsigned |
| Repository pubblico | Ruleset/branch protection prima di 1.0, scansioni automatiche e nessuna credenziale nel codice |

## Funzione incrementale: dimensioni cartelle (richiesta utente 30/10/2026)

Aggiunto nei sorgenti un calcolo ricorsivo della dimensione delle cartelle (solo righe visibili, 2 worker, cache, limiti 75.000 elementi / 3 secondi, disattivabile dal menu Visualizza). Il dato è riferito ai **byte logici**, non all'occupazione fisica; symlink, junction, directory non accessibili e cartelle fuori budget restituiscono «n/d» invece di un totale incompleto. Test backend aggiunti; **collaudo Windows della nuova interfaccia ancora aperto** (SIZE-01…07 in COLLAUDO_WINDOWS.md).

## Miglioramento incrementale: spazio libero delle unità (richiesta utente 30/10/2026)

La sidebar elenca unità locali e di rete mappate: implementata una prima lettura asincrona Windows dei byte disponibili all'utente per volume, con capacità totale nel tooltip, valori `n/d` per unità non disponibili, refresh con F5 e periodico, senza blocco del thread UI. Aggiunti test del wrapper Win32 e piano di collaudo DRIVE-01…07. **Implementazione non equivalente a collaudo GUI sulla macchina dell'utente.**

## M1, primo incremento (30/10/2026): move tra volumi per file regolari
È stato implementato nel **ramo draft** il fallback per `ERROR_NOT_SAME_DEVICE`: copia in staging, controllo byte per byte, pubblicazione senza overwrite ed eliminazione della sorgente *solo dopo* la verifica e solo se risulta invariata. Annullamenti/errori prima del commit lasciano intatta la sorgente; gli errori successivi al commit vengono riportati come esiti parziali, senza nascondere l'eventuale doppia copia.

**Secondo incremento implementato nel ramo, da verificare:** spostamento cartelle cross-volume con manifest sorgente, confronto esatto di struttura/cartelle vuote/file, verifica byte e pubblicazione senza overwrite. Solo dopo l'esito positivo viene tentato il trasferimento della sorgente nel Cestino Windows; se il Cestino non è utilizzabile, l'operazione resta parziale e la copia pubblicata viene conservata. Cancellazione diretta ricorsiva deliberatamente evitata. Rifiuta symlink/junction e alberi oltre 100.000 elementi. Mancano prove Cestino reale, unità rimovibili e share SMB, oltre a ACL/metadati speciali, undo e atomicità piena. I gate M1 non sono chiusi; casi `MOVE-X01…X10`.

## 7. Ordine consigliato delle prossime azioni

1. **Subito:** confermare su Windows reale l'ultima build `0.10.0-dev`, compilata con 68 test, e registrare i risultati nell'issue #13.
2. **Poi:** chiudere M0 e correggere ogni bug P0 emerso, prima di aggiungere funzionalità.
3. **Successivamente:** implementare spostamenti tra volumi e consolidare il motore file (M1), la UX (M2) e ricerca/anteprime (M3).
4. **Quindi:** misure di prestazioni/robustezza M4, freeze per RC, test M5 e release M6.
5. **Solo dopo la 1.0:** Windows Shell avanzata, comandi/Undo/ZIP, ricerca indicizzata, PDF grafico e sperimentazioni per 2.0.

### Tracciamento e governo
- Aprire una issue per ogni gruppo V2-xxx, raggruppata per milestone; una PR deve chiudere un insieme limitato di issue, non l'intera roadmap.
- Stati consigliati: `Da sviluppare`, `In lavorazione`, `Implementato/CI verde`, `Collaudato su Windows`, `Rilasciato`.
- Non fondere `feat/roadmap-to-1-0` solo perché è verde su GitHub Actions.
- Questa roadmap **non** modifica retroattivamente lo stato dei test della release: la fonte per i gate reali resta `RELEASE_CHECKLIST.md` e [issue #13](https://github.com/blackkiteo/Veldir/issues/13).
