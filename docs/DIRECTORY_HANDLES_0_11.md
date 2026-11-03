# 0.11 — guard delle directory sorgenti Windows

Base `c232afd1ee40259845dfac0592736d1b03e97887`, PR #21 non unita. Worktree/ramo isolato `fix/0-11-protected-directory-handles`. Ambito V2-014/V2-015/V2-039; M1 resta aperta.

## Contratto

Il copier apriva le directory per percorso dopo il controllo iniziale, senza mantenere un handle verificato durante l'enumerazione e la discesa nei figli. L'incremento aggiunge un guard per ogni directory sorgente Windows visitata: apertura `BACKUP_SEMANTICS | OPEN_REPARSE_POINT`, condivisione `FILE_SHARE_READ`, QoS anonimo e validazione sull'handle prima di creare la directory di output o chiamare `read_dir`.

Il guard rimane vivo durante enumerazione, discesa iterativa e controllo finale del tipo/data di modifica. Una pila DFS esplicita sullo heap conserva frame indipendenti con percorsi, enumeratore, guard e timestamp; gli antenati già visitati restano quindi aperti mentre si copiano i discendenti. L'enumerazione elabora una entry alla volta. Errori e annullamento rilasciano gli handle tramite RAII; una directory incompleta rimane nello staging e non viene pubblicata.

Il limite è di **256 guard directory simultanei**: la directory radice conta 1; un file non aggiunge un guard directory. La profondità viene controllata nel preflight globale e di nuovo nel copier, prima di aprire un ulteriore guard. Il buffer file da 256 KiB viene allocato sullo heap, evitando di riservarlo nei frame ricorsivi. Il budget preflight di 100.000 elementi e la contabilità stimata di 32 MiB di percorsi pendenti restano distinti.

## Garanzie limitate

Il punto di cattura resta l'apertura della singola directory. Non è certificata l'identità rispetto al preflight, né la stabilità degli antenati esterni alla radice copiata. `read_dir` e le aperture dei figli restano basati sul percorso; non sono operazioni relative a un handle verificato. Nessuna promessa di traversal interamente protetto o di snapshot/freeze delle entry. I metadata finali possono rilevare alcune mutazioni, non tutte.

Gli handle limitano le operazioni incompatibili sul relativo oggetto secondo lo sharing Windows. Le entry e i contenuti possono comunque cambiare fra operazioni: le sorgenti file mantengono le protezioni della #21. Il comportamento di ogni server SMB e filesystem USB richiede ancora collaudo reale. Le piattaforme non Windows non acquisiscono le garanzie di sharing Windows.

Riferimenti: [Microsoft CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew), [Rust OpenOptionsExt](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html). Nessun nuovo `unsafe`, dipendenza o tentativo di abilitare privilegi di backup.

## Collaborazione e verifiche

La prima [CI Windows 41364524858](https://github.com/blackkiteo/Veldir/actions/runs/41364524858), head `c438cd6`, ha superato formattazione e compilazione, ma il processo test è abortito per stack overflow nel thread `directory-depth-256`. Spostare il buffer non bastava. Nessun conteggio di suite superata; benchmark, release e packaging saltati. La correzione approvata da QA e Security sostituisce la ricorsione con frame DFS sullo heap, mantenendo invariati il limite 256 e il test su 2 MiB. Serve una nuova CI sullo SHA corretto; non usare la run fallita come evidenza positiva.

QA e Security hanno approvato il piano dopo consultazioni indipendenti, richiedendo di risolvere il buffer stack prima del limite di profondità. Transfer Safety è l'unico autore Rust nel worktree; il coordinatore cura documentazione, integrazione e CI. Nessun agente Performance o Release separato.

Regressioni richieste: directory sostituita con file o symlink prima apertura; rename/rimozione incompatibili rifiutati su root e directory annidata mentre il figlio viene copiato/sincronizzato; reader ammesso; rilascio dopo successo, cancellazione, errore enum/sync; directory vuote/Unicode; mutazione timestamp; limite esatto e oltre; profondità reale 256 su thread debug con stack impostato a 2 MiB. Symlink obbligatorio in CI, senza skip silenziosi.

Le fixture sono temporanee e sintetiche. Nessun dato personale, share reale o Cestino dell'utente. Formattazione, review finale e risultati Windows sullo SHA finale vengono registrati nella PR dopo esecuzione. Test Rust locali non eseguiti: linker MSVC assente già diagnosticato.

## Gate aperti

Cross-volume resta disabilitato. Identità precedente all'apertura, antenati esterni, traversal relativo a handle, ADS/ACL/metadati speciali e collaudo reale USB/SMB restano aperti. Nessuna chiusura automatica di M1, nessun merge, tag o release.
