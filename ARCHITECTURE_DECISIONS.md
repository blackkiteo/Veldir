# Decisioni per la distribuzione e le integrazioni
## ZIP portabile
Scelto per la build di sviluppo: nessun installer, modifica al registro o privilegio amministrativo. Il launcher Veldir-portable.cmd attiva una cartella VeldirData accanto all'EXE; l'avvio normale mantiene la posizione standard delle impostazioni.
## Firma digitale
Preparato scripts/sign-windows.ps1, da usare solo con un certificato del proprietario già disponibile nel certificate store e un servizio di timestamp scelto dal proprietario. Lo script firma, verifica e rigenera il checksum. Non è stato eseguito: nessun certificato è stato fornito. La firma va verificata sul pacchetto candidato prima della pubblicazione.
## PDF
Il renderer grafico richiederebbe un componente dedicato (es. PDFium da distribuire e aggiornare oppure API Windows PDF con gestione dei thread e delle risorse). Questa integrazione conserva l'estrazione testuale limitata e l'apertura nel visualizzatore associato. Non è un renderer di pagine; la valutazione non è dichiarata come implementazione.
## Menu Shell
Il menu contestuale Shell completo coinvolge estensioni di terzi e vincoli dei thread COM. Sono mantenute le azioni esplicite di Veldir, l'apertura tramite associazione Windows e la clipboard Shell già presente. L'integrazione di handler Shell completi richiede una modifica separata con test dedicati.
## Accessibilità
AccessKit è attivato; controlli e righe sono esposti a Windows. La presenza nell'albero UIA non certifica il funzionamento con screen reader. Focus, tastiera, contrasto e DPI richiedono le verifiche riportate in RELEASE_CHECKLIST.md.
