# Audit trasferimenti 0.11 — 1 novembre 2026

Base `37306fb4ae6372d43ddae54827c9abdccdb3f9d9`, ramo `feat/roadmap-to-1-0`, PR #12 draft. Riferimenti: V2-011…018, issue #13/#17. Intervento isolato: `fix/0-11-transfer-safety`.

## Agenti realmente eseguiti

Coordinatore: roadmap, integrazione, documentazione e CI. QA e Security: revisioni indipendenti read-only, poi revisione finale. Transfer Safety: unico autore di `src/transfer.rs` e `src/fs_ops.rs`, implementazione Rust/Windows e regressioni. Nessun agente separato Performance o Release avviato; nessuna nuova misura prestazionale dichiarata.

## Decisioni approvate dopo consultazione

| Priorità | Problema | Correzione |
| --- | --- | --- |
| P0 Security | Identità della sorgente basata su percorso/mtime/dimensione; race fra verifica e riciclo; ADS non preservati. | Fallback cross-volume disabilitato prima di staging/Cestino; prototipi solo nei test. Same-volume conservato. |
| P0 Security / P1 QA | `flush` non richiede persistenza; verifica può leggere dalla cache. | `sync_all` fallibile per ogni file staged prima della pubblicazione; fault injection. Nessuna garanzia universale su hardware/server. |
| P1 QA | Errore staging perde riepilogo di un batch già iniziato. | Errore per elemento nell'accumulo comune; regressione con destinazione rinominata dopo il primo file. |
| P1 QA | Collisione tardiva con Stop diventa KeepBoth. | Interruzione esplicita conservando sorgente e file concorrente; regressione con callback. |

QA e Security hanno approvato staticamente questo scope. Il test same-volume usa il wrapper di produzione. Gli agenti revisori non hanno eseguito test Rust.

## Limiti aperti e accettazione

M1 resta aperta. Prima di riabilitare cross-volume servono identità/handle, controllo concorrente, politica ADS/ACL/metadati e prove reali USB/SMB/Cestino. Restano P1: race reparse fra controllo e apertura anche nelle copie; scansioni preflight non cancellabili e latenza su share lente. Nessuna garanzia di snapshot coerente di sorgenti mutate esternamente. Il test storico sul cambio dopo pubblicazione prova metadata, non l'intera finestra di riciclo.

Nuovi test: gate file/cartella senza staging/riciclo; errore sync senza pubblicazione e origine integra; collisione tardiva Stop; riepilogo dopo destinazione indisponibile. Fixture sintetiche temporanee e riciclo simulato, nessun dato personale o Cestino reale.

Rust 1.95.0 installato in `.toolchain` esterna ai worktree senza cambiare PATH globale. Formattazione verificata. `cargo check --locked --all-targets` locale tentato: **non superato**, manca `link.exe` MSVC. Suite, benchmark e build Windows sullo SHA finale devono essere verificati in CI; risultati e SHA sono riportati nella PR dopo esecuzione.

CI baseline osservata tramite API: Windows `41356210333`, agent validation `41356210316`, secret scan `41356210302` success sul solo SHA base; non verificano questa patch. Nessun collaudo hardware, merge, tag o release effettuato.
