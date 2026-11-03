# 0.11 — preflight conservativo delle rinomine multiple

Base: `65dfd569cb3870ce9eb0b97222be5fed84e1b38c` (PR #22, ancora draft). Intervento circoscritto a `src/fs_ops.rs`; non modifica i trasferimenti cross-volume.

## Problema

`preflight_rename_many` controllava i nomi di destinazione duplicati, ma non la stessa sorgente indicata due volte con nuovi nomi diversi. L'esecuzione sequenziale poteva rinominare il file al primo passaggio e fallire al secondo, lasciando un batch parziale che il preflight avrebbe potuto rifiutare. Inoltre `symlink_metadata(target).is_ok()` trattava qualsiasi errore come se il target fosse libero; permessi e altri errori non garantiscono assenza.

## Contratto dopo la modifica

- Rifiutare prima di qualsiasi scrittura le sorgenti duplicate (con confronto testuale senza distinzione maiuscole/minuscole, coerente con gli altri controlli dei nomi Windows).
- Rifiutare il batch quando non è possibile leggere i metadata delle sorgenti.
- Verificare le destinazioni con `path_exists`: solo `NotFound` indica assenza; errori d'accesso, filesystem o rete provocano un errore, senza iniziare il batch.
- Conservare la protezione `rename_no_replace` durante l'esecuzione, poiché un nome libero al preflight può essere occupato in seguito.

## Test e limiti

Il test `bulk_rename_duplicate_source_fails_before_any_write` copre sia `execute(Job::RenameMany)` sia `execute_progress(Job::RenameMany)` e verifica che la sorgente e i due possibili target rimangano invariati. Il test esistente `bulk_rename_preflight_never_overwrites` continua a verificare collisioni preesistenti.

Questo **non** rende la rinomina multipla transazionale: errori e conflitti che insorgono *dopo* il preflight possono lasciare rinomine precedenti già completate e dichiarate nel messaggio d'errore. La deduplicazione confronta percorsi testuali e non certifica l'identità dei file attraverso alias, junction o cambiamenti concorrenti.

## Verifica richiesta

- `cargo fmt --all -- --check`
- `cargo check --locked --all-targets`
- `cargo test --locked --all-targets`, incluso il nuovo test
- Windows CI sul commit della PR e collaudo su fixture sintetiche

Nessun merge, tag, release o prova su file personali effettuato da questa modifica.
