# Squadra multi-agente Veldir

**Scopo:** continuare lo sviluppo di Veldir 0.11 → 1.0 → 2.0 con delega, verifica indipendente e coordinamento verificabile. Questa è un'infrastruttura **configurata**, non dieci processi sempre attivi.

## Ruoli e punti di contatto

| Agente | Ambito e responsabilità | Collabora principalmente con |
| --- | --- | --- |
| `veldir-orchestrator` | Roadmap, scomposizione, conflitti, integrazione | Tutti |
| `veldir-rust-core` | Architettura Rust, threading, stato, refactoring | transfers, QA, perf |
| `veldir-transfer-safety` | Copia/move, staging, Cestino, conflitti, annullamento | Windows, security, QA |
| `veldir-windows` | Win32, Shell, clipboard, USB/SMB/UNC, DPI e handle | transfer, UX, security |
| `veldir-ux` | egui, tastiera, pannelli, UX, accessibilità | Windows, QA |
| `veldir-search-preview` | Ricerca, watcher, cache, miniature, preview e PDF testuale | perf, UX, security |
| `veldir-qa` | Test deterministici, regressioni, fixtures, collaudo manuale | Tutti, security |
| `veldir-performance` | Profiling, cache e RAM/CPU, benchmark, latenza rete | core, UX, search |
| `veldir-security` | Revisione **sola lettura** di integrità, unsafe, permessi e supply chain | transfer, Windows, release |
| `veldir-release` | CI Windows MSVC, ZIP/checksum, changelog, documentazione, gate 1.0 | QA, security, orchestrator |

## Come collaborano realmente

1. **Subagenti** (Copilot/VS Code quando supportato): i file `.github/agents/*.agent.md` dichiarano il tool `agent` per consentire di chiedere una verifica a un altro ruolo. L'orchestratore sceglie 1–3 specialisti secondo il problema, acquisisce i risultati e decide.
2. **Deleghe annidate** in VS Code Local: `.vscode/settings.json` abilita `chat.subagents.allowInvocationsFromSubagents`. La funzione richiede un editor/versione che la supporti e il permesso per `Run Subagent`; è possibile disabilitarla nelle proprie impostazioni. Niente cicli di richieste: profondità di squadra raccomandata **2**; il runtime può imporre altri limiti.
3. **Handoff**: nei file agent sono presenti passaggi suggeriti verso i revisori successivi, quando supportati dall'interfaccia Local; sono interazioni deliberate, non chat autonome permanenti.
4. **Collaborazione asincrona tra sessioni**: usare issue e PR per persistere le richieste. Non presumere che citare un *nome di agente* in un commento GitHub costituisca un'assegnazione eseguibile. Serve avviare una sessione Copilot, delegare realmente o assegnare un ticket all'integrazione disponibile.
5. **Copilot cloud su GitHub.com**: le definizioni hanno frontmatter portabile; `agents` e `handoffs` possono essere ignorati dal runner cloud. I file restano validi per i ruoli, ma non garantiscono reciprocità runtime. Verificare l'ambiente reale e non inventare esecuzioni.

## Protocollo di richiesta reciproca

Per ogni passaggio usare il seguente messaggio (come task al subagente o commento tracciato):

```text
[VELDIR-ASK] from=veldir-transfer-safety to=veldir-windows
Ticket: #<numero> | Base: <branch e SHA>
Obiettivo: <decisione o verifica circoscritta>
Contesto: <file/funzioni rilevanti e test esistenti>
Vincoli: niente file reali, no overwrite, scope delimitato
Deliverable: analisi, patch proposta oppure test (specificare se read-only)
Scadenza/dependency: <prima di quale step/PR>
```

Rispondere con:

```text
[VELDIR-ANSWER] from=veldir-windows to=veldir-transfer-safety
Esito: APPROVE | CHANGES_REQUIRED | BLOCKED
Evidenze: file:line / test / CI / documentazione
Rischi: <conseguenze sui dati, UI, Win32, prestazioni>
Azioni: <1–3 passi o patch>
```

**Non** inviare richieste ricorsive indefinite: se un agente non può risolvere, segnala `BLOCKED` all'orchestratore. L'owner del ticket mantiene il contesto e coordina la fusione dei risultati.

## Workflow operativo per milestone

1. **Triage**: leggere `ROADMAP_V2.md`, PR #12, issue #13 (collaudi) e #15 (branch protection); aprire o utilizzare un issue dal template `agent-task.md`.
2. **Pianificazione**: orchestrator designa owner, revisori, criteri e ordine. Esempio: trasferimenti cross-volume → transfer-safety, windows, security e QA.
3. **Analisi parallela**: security e QA possono lavorare in sola lettura in parallelo; più agenti che modificano gli stessi file **non** lavorano in parallelo nello stesso worktree.
4. **Implementazione**: owner scrive piccole modifiche, test di regressione e documentazione pertinente su branch dedicato; nessun altro deve cambiare quegli stessi file senza coordinamento.
5. **Revisione incrociata**: security per effetti distruttivi/unsafe; QA per regressioni; Windows/UX/perf se interessati; correggere findings P0 prima di procedere.
6. **Gate CI**: aspettare **run sullo stesso SHA** del risultato proposto. Non confondere CI annullata per `concurrency` con passaggio riuscito.
7. **Validazione manuale**: issue #13 con ambiente, build, procedura e prova; mai segnare automaticamente gli scenari USB/SMB/Cestino/DPI/accessibilità come collaudati.
8. **Merge/release**: solo il proprietario decide. Nessuna promozione a 1.0 quando PR è draft o i gate non sono chiusi.

## Escalation e decisioni

- **P0 dati**: stop, preservare originale, consultare immediatamente transfer-safety + security + QA.
- **P0 Windows/unsafe**: stop, coinvolgere windows + security.
- **UI bloccata/SMB lento**: windows + performance + UX.
- **Regressione test**: QA riproduce, owner corregge, performance verifica se c'è impatto.
- **Pubblicazione**: release prepara evidenze; orchestrator rivede; solo il proprietario autorizza.

## Attivazione della squadra

- In **VS Code con Copilot**, aprire la repo, selezionare l'agente `veldir-orchestrator` e chiedere per esempio: «Coordina gli specialisti per il prossimo ticket P0 di ROADMAP_V2, produci piano e PR; non effettuare merge». Per deleghe annidate, abilitare il tool di subagente e la relativa impostazione.
- In **GitHub Copilot CLI/app**, scegliere un custom agent o il comando `/fleet` per sottotask indipendenti quando disponibili. Il supporto effettivo dipende dal prodotto/piano.
- L'installazione delle configurazioni **non** avvia daemon, schedulazioni, polling permanente o altri agenti; ciascuna esecuzione richiede una sessione avviata e le autorizzazioni del provider.
- Prima di accettare il sistema come operativo, eseguire un test di delega reale: orchestrator chiede a QA di cercare test per `src/transfer.rs`, a security di fare review read-only, e compone due risposte in un ticket, senza edit concorrenti.

**Riferimenti ufficiali:** [GitHub custom agents](https://docs.github.com/en/copilot/reference/custom-agents-configuration), [GitHub delega multi-agente](https://docs.github.com/en/copilot/concepts/agent-capabilities/choosing-and-delegating-agent-work/delegating-and-parallelizing-work-with-agents), [VS Code subagents](https://code.visualstudio.com/docs/agents/run/subagents).
