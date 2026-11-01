# Condizioni per Veldir 1.0

Versione candidata attuale: **0.10.0-dev**. Non è ancora una release stabile.
La CI prova compilazione, codice e scenari simulati; non certifica il comportamento dell'interfaccia su un PC Windows reale.

## Verifiche automatiche eseguite
- [x] Formattazione Rust, compilazione Windows MSVC, Cargo.lock e build riproducibile.
- [x] Test Rust e benchmark separato per cartelle con 1.000/10.000 elementi (solo backend).
- [x] Generazione ZIP portabile, metadati build e checksum SHA-256.
- [x] Controllo automatico segreti Gitleaks.
- [ ] Rieseguire tutti i controlli sul **commit esatto della release candidata** dopo l'ultima modifica.

## Collaudo manuale ancora necessario
- [ ] Ctrl+C/Ctrl+V/Ctrl+X fra pannelli Veldir e fra Esplora e Veldir; sostituzione della clipboard dopo un taglio; campo di testo protetto.
- [ ] Copia, spostamento, conflitti Interrompi/Salta/Conserva entrambi, annullamento, file bloccati e confronto dei contenuti.
- [ ] Mouse, selezione, trascinamento fra pannelli e riordino schede.
- [ ] Ripristino sessioni, percorsi mancanti, condivisioni UNC e unità rimovibili.
- [ ] DPI 100/125/150/200%, due monitor, mouse e screen reader.
- [ ] Prestazioni reali di EXE, RAM e CPU a riposo, dischi locali, rete e unità rimovibili.
- [ ] Revisione finale senza difetti critici noti, con esiti e prove annotate nell'issue #13.
- [ ] Decisione esplicita sulla firma digitale: richiede un certificato del proprietario oppure pubblicazione dichiaratamente non firmata.

## Scelte di distribuzione e limiti espliciti
- ZIP portabile, senza installer o privilegi amministrativi. `--portable` salva in `VeldirData` accanto all'EXE; `--path CARTELLA` sceglie il percorso iniziale.
- PDF: anteprima testuale e apertura esterna; **non** rendering delle pagine.
- Menu contestuale Shell completo e firma digitale **non** sono implementati.
- Nessun merge di PR #12 e nessuna release 1.0 finché i criteri richiesti non hanno evidenze verificabili.
