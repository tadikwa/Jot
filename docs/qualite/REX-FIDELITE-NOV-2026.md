# Retour d'expérience fidélité — novembre 2026

Ce rapport porte sur la branche `fix/fidelite-rex-nov-2026`, partie de `main` @ 7aa2a95 (1.7.1).

Environnement : Windows 11 Pro, RTX 5070 Ti, Rust 1.97.1, Qwen3-ASR 1.7B et Qwen3.5 4B (pack rapide) sur llama.cpp b12289.

Tous les exemples cités ici sont **synthétiques**. Les dictées de l'audit du 16 novembre 2026 ont été lues en local uniquement. Elles ne sont désignées que par leur identifiant (8 caractères). Aucun texte réel, aucun audio et aucun chemin utilisateur n'entre dans le dépôt (voir « Données personnelles »).

## Bilan comparatif (1.7.1 → branche)

| | 1.7.1 (`main`) | Branche |
|---|---|---|
| Incidents P0/P1 de l'audit refusés | 0 / 3 | 3 / 3 |
| Autres pertes réelles trouvées dans l'audit | 0 | 2 (784c761a, 06ba3aba) |
| Reformulations de l'audit refusées | 2 / 93 | 8 / 93 |
| Faux positifs sur l'audit | 0 | 1 (a0081fdf) |
| Références du bench refusées (garde seul) | 0 / 125 | 1 / 125 |
| Sorties réelles du bench acceptées | 119 / 125 | 118 / 125 |
| Sorties du modèle | — | identiques (125 / 125) |
| Latence médiane du bench | 0,191 s | 0,198 s (bruit) |
| E2E binaire réel, profil isolé | — | 3 / 3 insérées, diagnostic et dictionnaire OK |

**Améliorations.** Le garde refuse maintenant les trois incidents de l'audit : la négation inversée, la condition devenue concession et le « pas encore » perdu. Il refuse aussi deux autres pertes réelles qu'il laissait passer. La cause commune était l'exemption de correction, qui valait pour toute la dictée ; elle est maintenant locale.

**Régressions.** Deux refus de plus, chacun retombant sur le brut, qui reste fidèle :

- a0081fdf (audit) : faux positif, voir plus bas.
- `d-long-retour-produit` (bench) : refus justifié par la règle des nombres, discutable sur le fond, voir plus bas.

Aucune sortie acceptée ne devient fausse, et aucun texte inséré n'est moins fidèle qu'en 1.7.1.

**Limites restantes.** Négations implicites, dictées longues condensées, « un / une » lus comme des nombres et « ne » seul compté comme négation : détails dans « Non couvert ».

## Ce qui n'a pas changé

- Pas de nouveau modèle, backend, service ou réglage. Le prompt Qwen est inchangé.
- `validation.rs`, partagé avec Gemini, n'est pas modifié. `fidelity::check` n'est appelé que depuis `local::reformulation_verdict`, donc le mode Gemini ne change pas.
- Le repli ne change pas : une reformulation refusée insère le brut après passage du dictionnaire (`dictionary.correct(&raw)`). Il n'y a pas de second appel au modèle, et `local_partial_rescue` reste à `false`.
- Les diagnostics locaux (`diagnostic.json`) restent optionnels et soumis à leur rétention. Les journaux ne contiennent que le motif du refus, jamais le texte.
- L'API publique de `fidelity` (`check`, `explain`) est inchangée.

## Défauts, causes et corrections

La correction porte sur un seul fichier : `crates/jot-core/src/fidelity.rs`. C'est le garde de fidélité, il compare le brut et la reformulation.

### Cause commune : l'exemption de correction était globale

En 1.7.1, il suffisait d'un marqueur d'auto-correction n'importe où dans la dictée (« non », « enfin », « je veux dire »…) pour couper le contrôle des nombres et des négations sur tout le texte. Une seule correction excusait donc toutes les pertes.

**Correction.** L'exemption est maintenant locale :

- un marqueur ne retire que les 6 mots qui le précèdent ;
- « pas » suivi d'un nombre retire ce nombre (« ce n'est pas mille deux cents, c'est mille quatre cents »).

Ailleurs, le contrôle reste actif.

### P0

| Défaut (synthétique) | Cause | Correction | Test |
|---|---|---|---|
| « il sert pas à grand-chose » devient « sert à grand-chose », alors que d'autres « pas » restent dans le texte (91661d83) | Le garde se contentait de vérifier qu'une négation subsistait quelque part | Chaque négation du brut est reliée à deux mots voisins. Elle est perdue si la reformulation contient plus de paires de ces mots non niées que le brut. Cas exclus : les questions rhétoriques (« est-ce qu'on pourrait pas… ») et les mots niant (« peu », « ne… plus », « jamais ») | `rex_nov_2026_a_negation_lost_beside_others_is_rejected`, `rex_nov_2026_negations_kept_or_retracted_pass` |
| « il n'y a pas l'alerte s'il n'y a pas d'erreur » devient « bien qu'il n'y ait pas d'alerte » (70355dc8) | Aucun contrôle des « si » | Nouveau motif `condition_lost`. Le mot qui suit « si / s'il / if » doit rester sous une tournure conditionnelle (si, quand, sans, en cas de, éventuel, uniquement…). Ne comptent pas comme conditions : « je sais pas si », « même si », « comme si », « voir si », « genre si », « par exemple si » | `rex_nov_2026_a_dropped_condition_is_rejected` |
| « la un point sept et la un point sept point un » devient « 1.7 et 1.7 » (variante de 05ee43b7) | Le contrôle comparait des ensembles de valeurs : 1, 7 restaient présents | Une version pointée d'au moins 3 segments doit se retrouver telle quelle (`number_lost`) | `rex_nov_2026_a_dropped_version_segment_is_rejected` |
| « cinq minutes … vingt de temps » devient « 5 minutes … 20 minutes » (variante de b5a55eaa) | « minutes » figurait déjà dans le brut, donc l'unité n'était pas jugée ajoutée | Chaque unité est rattachée aux mots-nombres qui la précèdent directement. « 20 minutes » est refusé, car le brut n'a jamais dit « vingt minutes ». « 1 400 euros par mois » ne rattache pas 400 à « mois » | `rex_nov_2026_a_unit_is_checked_against_its_own_number` |

L'incident 05ee43b7 (1.7.1 → 1.7) et l'incident b5a55eaa étaient déjà refusés par la 1.7.1 dans leur forme d'origine. Les variantes ci-dessus passaient encore en 1.7.1. Elles sont désormais refusées.

### P1

| Défaut | Correction | Test |
|---|---|---|
| « pas encore envoyé » devient « envoyé » (f9d3ea95) | Nouveau motif `pending_lost` pour « pas encore », « toujours pas » et « not … yet ». L'état peut être reformulé (« pas encore confirmé » → « non confirmé ») tant qu'un mot qu'il gouverne reste nié | `rex_nov_2026_a_pending_status_must_survive` |
| « Notepad plus plus » devient « Notepad plus » (f9d3ea95) | Nouveau motif `name_changed` : « plus plus » doit survivre, ou devenir « ++ » | `rex_nov_2026_a_product_name_keeps_its_parts` |
| « Deuxième sujet, le budget » : une liste peut perdre l'ordinal qui numérote le discours | Un ordinal (deuxième à dixième) en tête de phrase n'est pas un nombre à conserver. Ailleurs (« au deuxième étage »), il l'est toujours | `rex_nov_2026_an_enumeration_ordinal_may_go` |
| Graphie Jot / JOT | Pas de code. La reconnaissance vocale produit « JOT », une entrée de dictionnaire JOT → Jot la corrige, y compris dans le texte de repli | — |
| Dictées longues condensées | **Non corrigé** (voir « Non couvert ») | — |

### Libellés de l'historique

Dans `crates/jot/src/views/history.rs`, trois libellés FR/EN sont ajoutés pour `pending_lost`, `condition_lost` et `name_changed`. Les 10 motifs que `fidelity::check` peut renvoyer ont désormais un libellé. Le test existant vérifie que chaque libellé est distinct et indique « transcription brute conservée ».

## Validation finale (18 novembre)

### 1. Les deux références du bench refusées

Le garde avait d'abord refusé 2 des 125 références écrites à la main. Elles ont été réexaminées une par une, sans adapter les tests au nouveau comportement.

**`c-long-450` : faux positif, corrigé.** La référence résume la dictée en liste. Deux règles la refusaient à tort :

- « Deuxième sujet », « troisième sujet » étaient lus comme les nombres 2 et 3 ;
- « pas encore confirmé » devenait « non confirmé », que `pending_lost` ne reconnaissait pas.

Les deux règles sont resserrées : l'ordinal de discours en tête de phrase et la reformulation qui garde la négation. Chacune a un test positif et un test négatif : « au deuxième étage » reste protégé, et « confirmé » tout court reste refusé. La référence est maintenant acceptée.

**`d-long-retour-produit` : refus gardé, discutable sur le fond.** La référence supprime la phrase de récapitulatif « ces deux ou trois petits soucis ». C'est la règle des nombres de la 1.7.1 : un nombre dit doit survivre. La 1.7.1 l'acceptait uniquement grâce à l'exemption globale, parce que la dictée contient ailleurs « enfin non ». Sans ce « enfin non », la 1.7.1 la refusait aussi. Sur le fond, la perte est bénigne. Une exception pour « deux ou trois » assouplirait le garde par rapport à la 1.7.1 pour de vrais comptes, d'où le choix de ne pas en faire. La sortie réelle du modèle pour ce cas perd davantage : « l'historique ne se vide pas » et « je ne sais pas » disparaissent. Le refus protège donc le texte inséré.

Le test `bench_references_pass_the_gate` fixe la liste exacte des refus : seul `d-long-retour-produit`.

### 2. Le faux positif a0081fdf

Le brut dit en substance « il n'y a même pas le menu », et la reformulation « il manque … le menu » garde le sens sans négation explicite.

Correctif essayé : compter « manque », « manquent », « absent(e) », « missing », « lacks » comme mots niant. Résultat sur l'audit : rien ne change, a0081fdf reste refusé. Dans la reformulation, « manque » ouvre une énumération et se trouve à une dizaine de mots de la paire « menu … propos », hors de la fenêtre de 5 mots. Pour l'accepter, il faudrait que « manquer » nie toute la phrase. Une vraie inversion (« il ne manque pas le menu » → « le menu est absent »…) pourrait alors passer dans une phrase longue. Le correctif est retiré, et a0081fdf reste un faux positif connu. Son coût : le brut est inséré à la place d'une reformulation correcte.

### 3. E2E avec le vrai binaire, les modèles locaux et un profil isolé

Le binaire est `target/release/jot.exe`, compilé depuis la branche, en copie portable. Le profil est isolé par le marqueur `jot-test-profile`, avec un dossier `profile` à côté de l'exécutable. Le microphone est remplacé par un WAV (`JOT_TEST_AUDIO`). Les modèles installés sont lus via des jonctions. La cible d'insertion est le Bloc-notes, piloté par `uia_probe dictate` (deux appuis sur Ctrl droit).

Le dictionnaire de test contient trois règles : Loostream → LooStream, firmware → micrologiciel, correctif → correctif LTS. L'audio est synthétique (SAPI, voix Hortense, 16 kHz mono).

| Dictée | Résultat dans le Bloc-notes | Délai |
|---|---|---|
| « Alors attends, j'ai pas encore envoyé le devis… » | Reformulation acceptée, « pas encore » conservé | 2,5 s |
| « …si le correctif est en ligne, et uniquement si… » | Reformulation refusée (`restriction_lost` : le modèle supprime « et uniquement si »). Brut inséré avec la règle « correctif LTS » appliquée. `diagnostic.json` écrit : motif, détail « condition restrictive disparue : « uniquement si » », proposition, texte inséré | 2,3 s |
| « Demain je présente le projet Loustream… » | Reformulation acceptée, terme du dictionnaire « LooStream » | 2,5 s |

L'historique du profil isolé contient 3 dictées `inserted`. Aucun mot dicté n'apparaît dans `jot.log`, `llama-asr.log` ni `llama-llm.log`. Le profil installé n'a pas été touché.

Le pipeline a aussi tourné sans le binaire, avec le test `real_engine_end_to_end` (ASR + LLM réels) sur 10 WAV synthétiques, avec et sans dictionnaire :

- les dix transcriptions sont correctes ;
- les quatre refus sont de vraies pertes du modèle : « et uniquement si » supprimé deux fois, « s'il n'y a pas d'erreur » devenu « sauf s'il y a une erreur », « je ne sais pas trop » devenu « à déterminer » ;
- rejouées dans le garde de la 1.7.1, ces 10 sorties donnent exactement les mêmes verdicts.

Non vérifié : l'interface à l'écran (Historique, page Dictionnaire), l'appli installée elle-même et la dictée au micro.

### 4. Intégration avec le travail 1.6 (dictionnaire intelligent, diagnostic des refus)

- `origin/release/v1.6.0-candidate`, `origin/dev` et les autres branches distantes n'ont aucun commit absent de `main`. Le dictionnaire intelligent (a9141b1, 0a6f166, b0fccef) et le diagnostic des refus (7578d63) sont dans `main`, donc à la base de la branche.
- La branche ne touche que `fidelity.rs`, un test ignoré dans `local.rs`, trois libellés de `history.rs` et ce rapport. Elle ne modifie ni `dictionary.rs`, ni `diagnostic.rs`, ni le chemin de repli.
- `fidelity::explain` remplit le détail du diagnostic pour les 10 motifs. L'E2E le montre sur `restriction_lost`.
- Les tests du dictionnaire et de l'apprentissage passent (suite complète, 0 échec). Le repli applique le dictionnaire, comme le montre l'E2E.

### 5. Données personnelles dans les fichiers poussés

Un script local mesure la plus longue suite de mots commune entre chaque texte ajouté par la branche et chaque dictée de l'audit (tests, rapport, libellés).

- Une chaîne de test reprenait une dictée de l'audit sur 27 mots : une liste de courses, d3187fa0. Elle a été remplacée par une liste synthétique (pique-nique) qui garde la même structure de correction. Les tests passent toujours.
- Recouvrement maximal restant : 11 mots, « on a peut-être gagné à peu près vingt de temps ». Cette phrase figure déjà dans `fidelity.rs` sur `main`. Tout le reste est à 8 mots ou moins, des tournures génériques.
- Le diff ne contient aucun chemin utilisateur, aucune adresse, aucun export, FLAC ou WAV. Il n'y a pas de fichier non suivi. Les scripts d'audit, les WAV et le profil E2E restent hors du dépôt.

## Mesures

### Audit (local, hors dépôt)

Le test ignoré `local::tests::fidelity_audit_replay` rejoue un export de l'Historique et n'affiche que des identifiants et des motifs. Il a été passé sur les 93 reformulations locales acceptées sur le terrain.

| | 1.7.1 | Branche |
|---|---|---|
| Refusées | 2 (05ee43b7 `number_changed`, b5a55eaa `unit_added`) | 8 |
| Cibles P0/P1 refusées | 0/3 | 3/3 : 91661d83 `negation_lost`, 70355dc8 `condition_lost`, f9d3ea95 `pending_lost` |
| Autres vraies pertes trouvées | — | 784c761a (négation perdue), 06ba3aba (« trois millions » → « des millions ») |
| Contrôles positifs 495d8029, d3187fa0, 926c3791 | acceptés | acceptés |
| Faux positifs | 0 | 1 : a0081fdf |

La première version de la branche produisait 6 fausses alertes sur l'audit (56d233d2, 0a60cea0, bf902976, dfc401b4, 1682920e, a0081fdf). Cinq ont été levées par des réglages ciblés, chacun couvert par un test synthétique : « pas » + nombre composé, « peu », « ne… plus », fenêtre de condition élargie, « éventuel(le)s ».

### Bench réel (`reformulation_bench`, Qwen3.5 4B, pack rapide)

Le bench a tourné avant et après sur les mêmes 125 cas, dans une racine isolée. Les sorties du modèle sont identiques (décodage glouton) ; les 125 sorties enregistrées ont été rescorées avec le garde final.

| | 1.7.1 | Branche |
|---|---|---|
| Acceptées | 119 / 125 | 118 / 125 |
| Latence médiane / p90 | 0,191 s / 0,498 s | 0,198 s / 0,515 s |

- Refus communs (6) : `d-ordre-ambigu`, `r2-retractation` (`hedge_lost`), `r5-condition`, `n-uniquement-repete` (`restriction_lost`), `r7-ip` (`number_changed`), `n07-ip` (`content_divergence`, dans `validation.rs`).
- Refus en plus (1) : `d-long-retour-produit` (`number_lost`), voir la validation 1.
- L'écart de latence vient du bruit de mesure. Le garde ne fait que des calculs sur le texte, sans E/S ni appel au modèle.

## Commandes et résultats

| Commande | 1.7.1 | Branche |
|---|---|---|
| `cargo fmt --all --check` | OK | OK |
| `cargo clippy --all-targets --locked -- -D warnings` | OK | OK |
| `cargo test --locked` | jot-core lib : 265 réussis, 4 ignorés | jot-core lib : 275 réussis, 5 ignorés ; jot-core intégration : 16 ; jot : 112 ; 0 échec |
| `reformulation_bench` (ignoré, modèles réels) | 119/125 | 118/125 |
| `fidelity_audit_replay` (ignoré, export privé) | 2/93 refusées | 8/93 refusées |
| `real_engine_end_to_end` (ignoré, 10 WAV synthétiques) | mêmes verdicts | 4 refus, tous de vraies pertes |
| Binaire portable, profil isolé, Bloc-notes | — | 3/3 insérées |

Le cinquième test ignoré est `fidelity_audit_replay`. Il demande un export privé et ne tourne jamais en CI.

## Compromis

- **Plus strict sur les résumés qui laissent tomber un compte.** Hors auto-correction et hors ordinal de discours, un nombre dit (> 1) doit survivre. Prix mesuré : 1 référence du bench sur 125 et 1 sortie réelle sur 125 retombent sur le brut.
- **Négations repérées par paires de mots.** Une heuristique, pas une analyse syntaxique. Elle peut refuser une paraphrase sans négation explicite : c'est le cas a0081fdf.
- **Conditions.** Seul le mot qui suit « si » est suivi. Une condition reformulée avec un tout autre vocabulaire et sans mot conditionnel est refusée. À l'inverse, une condition déplacée sur un autre mot peut passer.
- **Aucun réglage n'élargit ce que le modèle a le droit de faire.** Tous rendent le garde plus exigeant, ou corrigent une fausse alerte mesurée par un test positif et un test négatif.

## Non couvert

- **Dictées longues condensées.** Une garde de couverture (part des mots de contenu conservés) a été évaluée puis écartée. Les contrôles fidèles (0,45 / 0,50) et les cas suspects (0,66 / 0,52) se chevauchent, et des références correctes du bench descendent à 0,33, voire 0,00. Aucun seuil ne sépare ces groupes sans fausses alertes.
- **Négations implicites** (« manquer », « absent », « impossible ») dans la reformulation : a0081fdf reste un faux positif (validation 2).
- **« un » / « une » lus comme des nombres.** « sauf s'il y a une erreur » ou « dans un ordre à déterminer » sont refusés en `number_changed`. Le refus est juste sur le fond, mais le libellé est trompeur. La règle est antérieure à la branche.
- **« ne » seul compte comme négation.** « il sert pas à grand-chose » → « ne sert à grand-chose » est accepté (E2E). Le sens reste négatif, mais la phrase est bancale. La règle est antérieure à la branche.
- **Autres noms à éléments répétés.** Seul « plus plus » est protégé, pas « C sharp » ni « dot net ».
- Le bench n'a pas tourné avec `JOT_RESCUE=1` : le sauvetage partiel reste désactivé et n'est pas touché.
- L'interface n'a pas été vérifiée à l'écran (libellés de l'Historique), et aucune dictée n'a été faite au micro dans l'appli installée.

## Reproduire

```
cargo test --locked -p jot-core --lib fidelity
JOT_AUDIT_JSONL=<dictations.jsonl d'un export local> cargo test -p jot-core --lib fidelity_audit_replay -- --ignored --nocapture
JOT_LOCAL_E2E=<racine contenant local/models et local/runtime> JOT_REFORM_PACK=fast JOT_REFORM_CASES=<corpus séparés par ;> cargo test --locked -p jot-core --lib reformulation_bench -- --ignored --nocapture
JOT_LOCAL_E2E=<même racine> JOT_LOCAL_E2E_AUDIO=<dossier de WAV 16 kHz mono> cargo test --locked --release -p jot-core --lib real_engine_end_to_end -- --ignored --nocapture
```

E2E avec le binaire : copier `target/release/jot.exe` dans un dossier vide. Y créer un fichier `jot-test-profile` et un dossier `profile` contenant `settings.json`, `dictionary.json` et `local/{models,runtime}`. Fermer le Jot installé, qui refuse de cohabiter. Lancer `jot.exe --background` avec `JOT_TEST_AUDIO=<wav>`, puis `uia_probe <titre du Bloc-notes> dictate`.
