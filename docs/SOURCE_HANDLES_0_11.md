# 0.11 — apertura protetta delle sorgenti regolari Windows

Base `44eabcc5afbc04c427c2052c4086bbd49c669788`, PR #20 non unita. Worktree/ramo isolato `fix/0-11-protected-source-handles`. Ambito V2-014/V2-039/V2-040; M1 resta aperta.

## Contratto e punto di cattura

Il copier controllava `symlink_metadata` e poi apriva il percorso con `File::open`: l'ultimo componente poteva diventare un link o un oggetto di altro tipo fra queste operazioni. Per i file regolari Windows il motore usa un'apertura con `FILE_FLAG_OPEN_REPARSE_POINT` e condivisione `FILE_SHARE_READ`, senza condivisione scrittura/soppressione. I metadata vengono validati sull'handle prima di creare il file di output: reparse point, directory e tipi non supportati vengono rifiutati. Il contenuto viene letto da quello stesso handle, mantenuto fino alla sincronizzazione e ai controlli finali di dimensione, data di modifica e byte letti.

L'apertura è il punto di cattura della sorgente per questo incremento. Una sostituzione regular→regular precedente all'apertura può essere accettata, anche con dimensione/data uguali: non è una prova di identità rispetto al preflight o alla selezione iniziale. Gli identificatori acquisiti solo dopo l'apertura non risolverebbero quel problema.

L'helper imposta inoltre `security_qos_flags(SECURITY_ANONYMOUS)` tramite la libreria standard, senza fallback di riapertura meno protetto: limita l'impersonation se un percorso arbitrario viene sostituito con una named pipe. Non rende interrompibile un'apertura già bloccata e non sostituisce la validazione del tipo sull'handle.

La condivisione limita le normali aperture concorrenti incompatibili in scrittura e cancellazione/rinomina secondo il contratto Windows. Non è uno snapshot universale e non certifica modifiche tramite mapping o ogni server SMB. Le directory e i componenti antenati restano esposti alle race di percorso; `OPEN_REPARSE_POINT` sull'ultimo componente non costituisce traversal sicuro dell'intero percorso. Il fallback su altre piattaforme non acquisisce le garanzie di sharing Windows.

Riferimenti primari: [Rust OpenOptionsExt](https://doc.rust-lang.org/std/os/windows/fs/trait.OpenOptionsExt.html), [Microsoft CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew). Le verifiche di metadata sono riferite all'handle effettivamente aperto, non alla continuità dell'identità del percorso.

## Verifiche e ownership

QA e Security hanno svolto consultazioni indipendenti prima dell'implementazione e approvato staticamente la patch finale. Transfer Safety è l'unico autore di `src/fs_ops.rs` e del solo adeguamento autorizzato del test in `src/transfer.rs` nel worktree dedicato; il coordinatore cura documentazione e integrazione. Nessun agente Performance/Release separato.

Nove nuovi test automatici su fixture sintetiche: sostituzione con directory prima dell'apertura; contratto regular→regular; writer incompatibile già aperto; reader compatibile e writer/rinomina incompatibili dopo apertura e durante sync; Unicode/file vuoto; rilascio handle dopo successo/errore/annullamento; mutazione timestamp rilevata; annullamento prima output e symlink finale. Il test symlink Windows è obbligatorio, senza `ignore` o ritorni silenziosi: se i privilegi mancano, fallisce esplicitamente e il risultato deve essere registrato. Restano attive le regressioni sync/no overwrite/staging.

Il vecchio test del prototipo cross-volume è stato reso deterministico: modifica la fixture dopo la copia staged e prima della verifica, a handle già chiuso. I nuovi test del copier verificano separatamente il rifiuto delle modifiche durante lettura e sync. Non confondere i due confini temporali.

Formattazione, revisione finale QA/Security e CI Windows sullo SHA finale sono riportate nella PR dopo l'esecuzione. Il linker MSVC locale è assente: nessun test Rust locale dichiarato passato. Non si usano dati personali, share reali o Cestino dell'utente.

## Gate residui

Cross-volume resta disabilitato. Identità prima dell'apertura, traversal e handle delle directory, ADS/ACL/metadati, albero modificato durante la copia e collaudo reale USB/SMB restano aperti. Il preflight rimane limitato e cancellabile, ma non è uno snapshot. Nessun merge, tag o release.
