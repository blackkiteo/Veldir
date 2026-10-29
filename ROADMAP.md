# Stato della roadmap verso Veldir 1.0

Versione di integrazione: 0.10.0-dev. Nessuna fase è considerata certificata soltanto perché il codice è presente.

| Fase | Codice implementato | Verifiche ancora aperte |
|---|---|---|
| Consolidamento | Restyling, viewport, pannelli, filtri adattivi, errori recuperabili | Collaudo GUI completo e revisione PR #11 |
| Build | Cargo.lock, --locked, fmt --check, cache CI, stop build duplicate push/PR, build ID, checksum e metadati | Verificare pipeline MSVC e artifact finale |
| Elenco/tastiera | Colonne opzionali/ridimensionabili e persistenti; frecce/Home/Fine/Pagina, Maiusc, typeahead, scorrimento alla riga | Mouse, DPI e screen reader |
| Operazioni | Conflitti Stop/Skip/KeepBoth, staging, riepiloghi per elemento, storico limitato, avanzamento e rinomina con anteprima | Spazio esaurito, disconnessione, permessi e collaudo interattivo; stesso volume per spostare |
| Sessioni | Schede duplicabili/riordinabili/riapribili, elenco schede, pannello opposto, sessione opzionale, percorsi mancanti conservati | Riavvio su rete e unità rimovibili, migrazione su installazioni reali |
| Ricerca | Ambito editabile, filtro/ordine risultati, cancel, risultati precedenti conservati, apri posizione e seleziona | Prove con condivisioni lente e file spostati durante la ricerca |
| Anteprime/perf | Zoom/adatta, UTF-16, cache limitate, generazioni miniature, benchmark 1k/10k | RAM/CPU a riposo, prestazioni rete/removibili; PDF grafico da valutare separatamente |
| Integrazione/distribuzione | AccessKit, etichette righe, About/scorciatoie, ZIP portabile e impostazioni separate | Screen reader, firma con certificato del proprietario, menu contestuale Shell completo |
| Release 1.0 | Checklist e criteri di accettazione definiti | Tutti i criteri RELEASE_CHECKLIST.md prima di chiamarla stabile |

Scelte di questa fase: distribuzione portabile invece di installer; conflitti senza sovrascrittura; PDF testuale e apertura esterna. Queste scelte sono esplicite e non equivalgono a un renderer PDF o al menu Shell completo.

Il tentativo precedente di cattura GUI Windows è terminato in timeout. Nessun risultato manuale è stato inventato. Nessun merge o pubblicazione della 1.0 viene eseguito senza i criteri richiesti.

## Misura locale del backend di elenco
Windows, Rust 1.95 GNU, cartelle temporanee appena create, file piccoli, tre letture consecutive: 1.000 file [2,2,2] ms; 10.000 file [20,20,20] ms. Esclude creazione, UI, icone, miniature e rete. Non è un benchmark del tempo totale di apertura GUI.

## Completamenti di consolidamento
Recupero impostazioni da backup valido e avviso sui valori predefiniti, indicazione del file corrente, azioni contestuali sulle cartelle, contrasto secondario e accento verificati nei due temi. L’albero accessibile Windows della build locale espone pulsanti, campi e righe; cattura e geometria per i clic restano indisponibili (timeout anche dopo recupero). Non è una validazione di screen reader o interazioni GUI.
