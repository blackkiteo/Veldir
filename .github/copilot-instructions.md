# Veldir Copilot instructions

Leggi `AGENTS.md` e `docs/AGENT_TEAM.md` prima di pianificare o modificare codice. Tutti gli agenti condividono quelle regole di sicurezza, ticketing e collaborazione.

- Stack Windows/Rust/egui; privilegia integrità di file e responsività UI.
- Non trasformare successi CI in prove manuali; non fondere la PR #12 draft né pubblicare 1.0.
- Delega ai ruoli `.github/agents/*.agent.md` se lo strumento `agent` è disponibile; consulta `veldir-security` e `veldir-qa` per operazioni che possono cancellare, sostituire o spostare dati.
- Usa il protocollo `[VELDIR-ASK]` / `[VELDIR-ANSWER]` per richieste reciproche.
- Concludi riportando file, test eseguiti, commit/SHA, rischi aperti e prossima azione.
