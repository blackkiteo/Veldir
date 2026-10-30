# Stato della roadmap verso Veldir 1.0

Versione di integrazione: 0.10.0-dev. Nessuna fase è considerata certificata soltanto perché il codice è presente.

| Fase | Codice implementato | Verifiche ancora aperte |
|---|---|---|
| Consolidamento | Restyling, viewport, pannelli, filtri adattivi, errori recuperabili; PR #11 integrata in main | Collaudo GUI completo su Windows reale |
| Build | Cargo.lock, --locked, fmt --check, cache CI, stop build duplicate push/PR, build ID, checksum e metadati; CI Windows con 60 test + benchmark e ZIP verificati prima del consolidamento clipboard | Rieseguire la CI sul commit finale della candidata 1.0 |
| Elenco/tastiera | Colonne opzionali/ridimensionabili e persistenti; frecce/Home/Fine/Pagina, Maiusc, typeahead, scorrimento alla riga; gestione eventi nativi Ctrl+C/X/V, priorità clipboard Windows e pulsante Incolla coerente | Mouse, DPI, screen reader e test pratico della clipboard tra Veldir ed Esplora |
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

## Stabilizzazione dopo i primi test della build portabile (30 ottobre 2026)
- La prova sul PC dell'utente ha identificato il mancato funzionamento di Ctrl+C/Ctrl+V nella prima build: corretti gli eventi clipboard che egui-winit produce al posto dei normali keydown.
- Migliorata la priorità degli appunti: un Ctrl+X locale non prevale su nuovi appunti di Esplora o su testo copiato altrove; un Ctrl+C interno la cui esportazione Windows fallisce rimane disponibile finché non cambia la generazione degli appunti.
- Il pulsante Incolla può importare anche un elenco di file CF_HDROP già presente in Windows, senza dover prima selezionare file in Veldir. I trasferimenti restano soggetti a conferma e a politiche senza sovrascrittura.
- Aggiunti test di regressione per la validità del taglio locale e il rilascio del tasto V; collaudo interattivo Windows ancora da documentare. **Nessuna approvazione della release 1.0 implicita**.

## Verifica formato clipboard Windows (30 ottobre 2026)
- L'esportazione CF_HDROP ora serializza il blocco DROPFILES in una funzione separata, con intestazione, nomi UTF-16 e terminatore MULTI_SZ verificabili senza toccare la clipboard di sistema.
- Aggiunto un test nativo su Windows che consegna il blocco a `DragQueryFileW` e verifica due percorsi, uno con spazi e caratteri Unicode. Aggiunti rifiuti testati per input vuoto, file inesistenti e più di 500 percorsi.
- Prima di `EmptyClipboard`, Veldir controlla che la finestra in primo piano appartenga al proprio processo, per evitare di attribuire il contenuto a un'altra applicazione.
- La verifica del payload non sostituisce il test manuale di Ctrl+C/Ctrl+V con Esplora: i formati clipboard proprietari o virtuali non sono supportati.
