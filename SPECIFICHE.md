# Veldir — specifiche tecniche v0.5.0

## Ambito
Windows 10/11 x64. UI Rust + eframe/egui `=0.37.0` (renderer OpenGL/glow). Prototipo standalone senza Electron, account, telemetria o sincronizzazione in rete integrata. Licenza MIT.

## Architettura
- `src/main.rs`: bootstrap e finestra.
- `src/app.rs`: stato dei pannelli, schede, UI egui, persistenza, messaggi asincroni, selezione, drop Windows.
- `src/fs_ops.rs`: lettura della directory, operazioni su filesystem, preflight dei batch, staging, pubblicazione senza sostituzione, Cestino.
- Lettura directory e job di filesystem in thread separati; messaggi consegnati alla UI tramite `mpsc` e repaint su completamento.
- Pannello sinistro con più schede; pannello destro indipendente con un solo percorso; uno dei due è attivo.
- Selezione con `BTreeSet<PathBuf>`, anchor e ordine dei file visibili dopo filtro.
- Su Windows lo spostamento e la pubblicazione delle copie usano `MoveFileExW` con flag 0, senza `MOVEFILE_REPLACE_EXISTING`.
- Copia di file/cartelle in staging temporaneo presso la cartella target, con rifiuto dei symlink e dei punti di reindirizzamento.
- Operazioni su più file eseguite singolarmente dopo un preflight collettivo, non come singola transazione.

## Funzionalità implementate nei sorgenti
- Navigazione di cartelle con cronologia e schede.
- Due pannelli con percorsi indipendenti e focus.
- Selezione multipla Ctrl/Maiusc e filtro locale.
- Nuova cartella, rinomina, copia, spostamento sullo stesso volume.
- Batch con controllo preventivo dei conflitti.
- Drop di file dall'esterno con conferma di copia.
- Cestino con conferma, preferiti e sessione persistente.
- Controlli automatici Windows con GitHub Actions.
- Fix v0.2.1: ID di focus stabili, deselezione automatica al cambio filtro, blocco scorciatoie quando i dialoghi sono attivi.
- v0.3.0: pannelli ridimensionabili con ratio persistente, barra laterale collassabile, breadcrumb, ordinamento per dimensione/data, Ctrl+A ed Esc.

## Limiti di sicurezza
Le operazioni batch non sono transazionali: se la destinazione cambia durante l'esecuzione, alcune copie o spostamenti potrebbero già essere completati. Nessuna sovrascrittura automatica, nessun ripristino universale. Copie senza garanzia di conservare ACL, timestamp o metadati. Il Cestino può essere indisponibile su unità di rete. Non usare su dati importanti prima dei collaudi manuali.

## Aggiornamento 0.5.0
- Byte counters e annullamento durante copia con buffer di 256 KiB.
- Stage temporaneo con ripulitura su errore/annullamento, pubblicazione no-replace.
- Importazione esplicita dei file dalla clipboard CF_HDROP di Windows, senza cancellazione delle sorgenti.

## Roadmap
1. **0.2.1:** correzione focus e selezione, verifica CI Windows. Collaudo GUI ancora da eseguire.
2. **0.3:** divisore trascinabile, breadcrumb, ordinamento e sidebar collassabile.
3. **0.4:** barra avanzamento/annullamento, clipboard Windows, drag & drop interno, anteprime e icone native.
4. **1.0:** benchmark ripetibili, release firmata e distribuzione.
