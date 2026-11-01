# 0.11 — entrypoint di copia con preflight comune

Base: `ce99ae003def6644f864564a4e2000a5a172687a`, PR #19 non unita. Ramo isolato `fix/0-11-unified-copy-preflight`. Ambito V2-010/V2-014/V2-015; M1 resta aperta.

## Problema e comportamento

Il percorso diretto `fs_ops::execute(Job::Copy/CopyMany)` aveva un copier separato: poteva iniziare le scritture senza la scansione globale limitata introdotta dalla #19. In particolare un secondo elemento non leggibile poteva essere scoperto dopo la pubblicazione del primo.

Entrambi gli entrypoint di copia ora delegano al motore `execute_progress`, passando l'intero batch. Il limite di 100.000 elementi include tutte le radici/cartelle/file e la contabilità stimata dei percorsi pendenti è limitata a 32 MiB. Un errore del preflight ferma le scritture prima dello staging; un errore runtime conserva il conteggio dei completamenti precedenti. I controlli durante la copia, `sync_all` fallibile e pubblicazione senza overwrite restano nel motore comune.

L'API sincrona usa un token locale non annullato e un callback nullo: non acquisisce una nuova capacità di annullamento esterno. Chi necessita di annullamento/progresso deve usare `execute_progress`. Le chiamate filesystem già bloccate restano soggette ai tempi del sistema.

## Collaborazione e verifiche

QA e Security: consultazioni indipendenti e revisione finale in sola lettura. Transfer Safety: unico autore di `src/fs_ops.rs` nel worktree dedicato. Coordinatore: documentazione, integrazione e verifica CI. Nessun agente Performance o Release separato.

Regressioni richieste usando direttamente `execute`: limite della copia singola, limite condiviso fra più radici, sorgente successiva mancante/errore di enumerazione senza prima pubblicazione, successo al limite con scansione unica, collisioni senza overwrite, assenza di staging residuo. Fixture temporanee sintetiche e hook solo test, nessun dato personale, share o Cestino reale.

Formattazione e CI Windows sullo SHA finale sono registrate nella PR dopo esecuzione. Rust locale disponibile, linker MSVC assente: nessuna compilazione o test locale dichiarato passato.

## Limiti

Il consolidamento elimina un bypass interno, non chiude le race di identità/reparse, la conservazione ADS/ACL/metadati o il collaudo reale USB/SMB. Il preflight non è uno snapshot; un albero può cambiare dopo la scansione. Move cross-volume resta disabilitato. Nessun merge, tag o release.
