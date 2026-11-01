# 0.11 — preflight cancellabile e limitato

Base: `acb7ea46a8ba030cb38b7360e6d1753d99f40af2` (PR #18, non unita). Ramo dedicato: `fix/0-11-cancellable-preflight`. Ambito: V2-010, V2-014, V2-015, V2-018; nessuna chiusura automatica di M1.

## Contratto approvato

QA e Security hanno svolto consultazioni indipendenti in sola lettura. Il coordinatore ha approvato un preflight globale delle copie: un errore di scansione, un limite superato o l'annullamento fermano il batch prima dello staging. Cambiamento intenzionale rispetto al precedente comportamento che ignorava alcune sorgenti illeggibili nel totale e poteva copiare le altre. Gli errori sorti durante l'esecuzione continuano a conservare il riepilogo degli elementi già completati.

Il budget condiviso conta al massimo 100.000 elementi (radici, cartelle e file), con limite separato di 32 MiB per i percorsi pendenti, verificato prima dell'inserimento. Il preflight controlla l'annullamento fra operazioni e durante l'enumerazione. Una chiamata filesystem/SMB già bloccata non viene interrotta forzatamente: nessuna promessa di latenza massima.

I totali in byte delle copie sono stime ottenute prima delle scritture. Il riutilizzo della pianificazione evita scansioni annidate e riletture della destinazione dopo pubblicazione; non dimostra che la sorgente sia immutata. Il copier mantiene i controlli symlink/reparse, l'annullamento, `sync_all` fallibile e la pubblicazione senza overwrite. Gli spostamenti tramite rename sullo stesso volume avanzano per elementi, senza scansione ricorsiva per misurare i byte.

Gli elementi già conflittuali con politica Salta sono esclusi dalla scansione ricorsiva e rimangono saltati anche se il conflitto scompare durante il job. Questo impedisce di copiare elementi mai preflightati. Errori di accesso ai percorsi non equivalgono a destinazioni assenti. Nessuna scansione segue link o junction.

Il nuovo scanner globale si applica ai trasferimenti dell'app e alle copie tramite `execute_progress`. Il percorso legacy diretto `execute(Job::Copy/CopyMany)` rimane fuori da questo scanner: non dichiarare il limite come garanzia di ogni API interna. La contabilità di 32 MiB riguarda i percorsi pendenti dello scanner, non la RAM totale del processo o le capacità riservate dall'allocatore.

## Ownership e verifiche

Agenti realmente eseguiti: QA e Security per consultazione e review; Transfer Safety unico autore di `src/fs_ops.rs` e `src/transfer.rs` nel worktree dedicato. Coordinatore autore della documentazione e integratore. Nessun agente Performance o Release separato; nessun miglioramento numerico prestazionale dichiarato.

Test richiesti: annullamento prima e durante scansione/enumerazione; budget condiviso fra radici ed esattamente al limite; directory larga e memoria pendente limitata; errori metadata/enumerazione espliciti; nessuna scrittura su fallimento globale; totali e conteggi; Skip; errori runtime dopo primo completamento; regressioni sync/conflitti/cross-volume.

Le prove automatiche usano fixture temporanee sintetiche e punti d'iniezione deterministici, senza rete, dati personali o Cestino reale. Formattazione e CI Windows sullo SHA finale sono riportate nella PR dopo esecuzione. L'ambiente locale dispone di Rust 1.95 ma manca il linker MSVC: non dichiarare test locali passati.

## Gate ancora aperti

Move cross-volume resta disabilitato. Race di identità/reparse fra controllo e apertura, ADS/ACL/metadati e collaudo reale USB/SMB restano fuori da questo incremento. I limiti del preflight non costituiscono uno snapshot o un limite garantito dell'albero modificato esternamente durante la copia. Nessun merge, tag o release.
