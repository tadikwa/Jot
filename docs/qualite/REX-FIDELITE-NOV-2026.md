# Retour d'expérience fidélité — novembre 2026

Ce rapport porte sur la branche `fix/fidelite-rex-nov-2026`, partie de `main` @ 7aa2a95 (1.7.1).

Environnement : Windows 11 Pro, RTX 5070 Ti, Rust 1.97.1, Qwen3.5 4B (pack rapide) sur llama.cpp b12289.

Tous les exemples cités ici sont **synthétiques**. Les dictées de l'audit du 16 novembre 2026 ont été lues en local uniquement et ne sont désignées que par leur identifiant (8 caractères). Aucun texte réel, aucun audio et aucun chemin utilisateur n'entre dans le dépôt. Un script local a mesuré le recouvrement entre les exemples de test et les dictées. Deux exemples dépassent 8 mots consécutifs en commun avec une dictée : la liste de courses (un script de test déjà public dans `docs/v15`) et la phrase « vingt de temps » (déjà présente dans `fidelity.rs` à HEAD). Tous les autres restent à 8 mots ou moins.

## Ce qui n'a pas changé

- Pas de nouveau modèle, backend, service ou réglage. Le prompt est inchangé.
- `validation.rs`, partagé avec Gemini, n'est pas modifié. `fidelity::check` n'est appelé que depuis `local::reformulation_verdict`, donc le mode Gemini ne change pas.
- Le repli ne change pas : une reformulation refusée insère le brut après passage du dictionnaire (`dictionary.correct(&raw)`). Il n'y a pas de second appel au modèle, et `local_partial_rescue` reste à `false`.
- Les diagnostics locaux (`diagnostic.json`) restent optionnels et soumis à leur rétention. Les journaux ne contiennent que le motif du refus, jamais le texte.

## Défauts, causes et corrections

La correction porte sur un seul fichier : `crates/jot-core/src/fidelity.rs`. C'est le garde de fidélité, il compare le brut et la reformulation.

### Cause commune : l'exemption de correction était globale

Avant, il suffisait d'un marqueur d'auto-correction n'importe où dans la dictée (« non », « enfin », « je veux dire »…) pour couper le contrôle des nombres et des négations sur tout le texte. Une seule correction excusait donc toutes les pertes.

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
| « cinq minutes … vingt de temps » devient « 5 minutes … 20 minutes » (variante de b5a55eaa) | « minutes » figurait déjà dans le brut, donc l'unité n'était pas jugée ajoutée | Chaque unité est rattachée au nombre qui la précède. « 20 minutes » est refusé, car le brut n'a jamais dit « vingt minutes ». Une unité sans nombre juste avant (« 1 400 euros par mois ») n'est rattachée à rien | `rex_nov_2026_a_unit_is_checked_against_its_own_number` |

L'incident 05ee43b7 (1.7.1 → 1.7) et l'incident b5a55eaa étaient déjà refusés par la 1.7.1 dans leur forme d'origine. Les variantes ci-dessus passaient encore à HEAD. Elles sont désormais refusées.

### P1

| Défaut | Correction | Test |
|---|---|---|
| « pas encore envoyé » devient « envoyé » (f9d3ea95) | Nouveau motif `pending_lost` pour « pas encore », « toujours pas » et « not … yet » | `rex_nov_2026_a_pending_status_must_survive` |
| « Notepad plus plus » devient « Notepad plus » (f9d3ea95) | Nouveau motif `name_changed` : « plus plus » doit survivre, ou devenir « ++ » | `rex_nov_2026_a_product_name_keeps_its_parts` |
| Graphie Jot / JOT | Pas de code. La reconnaissance vocale produit « JOT », une entrée de dictionnaire JOT → Jot la corrige, y compris dans le texte de repli | — |
| Dictées longues condensées | **Non corrigé** (voir « Non couvert ») | — |

### Libellés de l'historique

Dans `crates/jot/src/views/history.rs`, trois libellés FR/EN sont ajoutés pour `pending_lost`, `condition_lost` et `name_changed`. Le test existant vérifie que chaque libellé est distinct et indique « transcription brute conservée ».

## Mesures

### Faux négatifs et faux positifs sur l'audit (local, hors dépôt)

Le test ignoré `local::tests::fidelity_audit_replay` rejoue un export de l'Historique et n'affiche que des identifiants et des motifs. Il a été passé sur les 93 reformulations locales acceptées sur le terrain.

| | HEAD 7aa2a95 | Branche |
|---|---|---|
| Refusées | 2 (05ee43b7 `number_changed`, b5a55eaa `unit_added`) | 8 |
| Cibles P0/P1 refusées | 0/3 | 3/3 : 91661d83 `negation_lost`, 70355dc8 `condition_lost`, f9d3ea95 `pending_lost` |
| Autres vraies pertes trouvées | — | 784c761a (négation perdue), 06ba3aba (« trois millions » → « des millions ») |
| Contrôles positifs 495d8029, d3187fa0, 926c3791 | acceptés | acceptés |
| Faux positifs | 0 | 1 : a0081fdf, voir ci-dessous |

La première version de la branche produisait 6 fausses alertes sur l'audit (56d233d2, 0a60cea0, bf902976, dfc401b4, 1682920e, a0081fdf). Cinq ont été levées par des réglages ciblés, chacun couvert par un test synthétique :

- « pas » + nombre composé ;
- « peu » ;
- « ne… plus » ;
- fenêtre de condition élargie ;
- « éventuel(le)s ».

Il reste a0081fdf, refusé par prudence. Le brut dit en substance « il n'y a même pas le menu », et la reformulation « il manque le menu » garde le sens sans négation. Le garde ne sait pas lire « manquer » comme une négation, et le texte brut est inséré à la place.

### Références du bench (garde seul)

Le test `fidelity::bench_references::bench_references_pass_the_gate` passe les 125 références écrites à la main de 7 corpus dans le garde.

| | HEAD | Branche |
|---|---|---|
| Références refusées | 0 / 125 | 2 / 125 |

Les deux refus sont connus, et le test les vérifie un par un :

- `c-long-450` est un résumé en liste qui laisse tomber « deuxième / troisième sujet ».
- `d-long-retour-produit` laisse tomber le récapitulatif « ces deux ou trois petits soucis ».

Ces deux refus viennent de la fin de l'exemption globale. Une reformulation de ce genre retombe sur le brut, qui reste fidèle.

### Bench réel (`reformulation_bench`, Qwen3.5 4B, pack rapide)

Le bench a tourné avant et après, sur les mêmes 125 cas, dans une racine isolée (jonctions vers les modèles installés).

| | HEAD | Branche |
|---|---|---|
| Sorties du modèle identiques | — | 125 / 125 (décodage glouton) |
| Acceptées | 119 / 125 | 118 / 125 |
| Latence médiane / p90 | 0,191 s / 0,498 s | 0,198 s / 0,515 s |

- Le seul refus en plus est `d-long-retour-produit` (`number_lost`). La sortie du modèle supprime le récapitulatif « deux ou trois soucis », ainsi que « l'historique ne se vide pas » et un « je ne sais pas ». Le brut est inséré.
- L'écart de latence vient du bruit de mesure. Les sorties sont identiques, et le garde ne fait que des calculs sur le texte, sans E/S ni appel au modèle.
- Le bench a d'abord tourné avant les deux derniers réglages : rattachement d'unité, puis « genre si ». Ces réglages ne touchent que le garde, et les 125 sorties enregistrées ont été rescorées avec le garde final. C'est le chiffre donné dans le tableau.

## Commandes et résultats

| Commande | HEAD | Branche |
|---|---|---|
| `cargo fmt --all --check` | OK | OK |
| `cargo clippy --all-targets --locked -- -D warnings` | OK | OK |
| `cargo test --locked` | jot-core lib : 265 réussis, 4 ignorés | jot-core lib : 274 réussis, 5 ignorés ; jot-core intégration : 16 ; jot : 112 ; 0 échec |
| `cargo test -p jot-core --lib reformulation_bench -- --ignored` (variables `JOT_LOCAL_E2E`, `JOT_REFORM_PACK=fast`, `JOT_REFORM_CASES`) | 119/125 | 118/125 |
| `JOT_AUDIT_JSONL=<export local> cargo test -p jot-core --lib fidelity_audit_replay -- --ignored --nocapture` | 2/93 refusées | 8/93 refusées |

Le cinquième test ignoré est `fidelity_audit_replay`. Il demande un export privé et ne tourne jamais en CI.

## Compromis

- **Plus strict sur les résumés qui laissent tomber un compte.** Hors auto-correction, un nombre dit (> 1) doit survivre. Prix : 2 références du bench sur 125 et 1 sortie réelle sur 125 retombent sur le brut.
- **Négations repérées par paires de mots.** Une heuristique, pas une analyse syntaxique. Elle peut refuser une paraphrase sans négation explicite (« manquer », « absent ») : c'est le cas a0081fdf.
- **Conditions.** Seul le mot qui suit « si » est suivi. Une condition reformulée avec un tout autre vocabulaire et sans mot conditionnel est refusée. À l'inverse, une condition déplacée sur un autre mot peut passer.
- **Aucun réglage n'élargit ce que le modèle a le droit de faire.** Tous rendent le garde plus exigeant, ou corrigent une fausse alerte mesurée.

## Non couvert

- **Dictées longues condensées.** Une garde de couverture (part des mots de contenu conservés) a été évaluée puis écartée. Les contrôles fidèles (0,45 / 0,50) et les cas suspects (0,66 / 0,52) se chevauchent, et des références correctes du bench descendent à 0,33, voire 0,00. Aucun seuil ne sépare ces groupes sans fausses alertes.
- **Négations implicites** (« manquer », « absent », « impossible ») dans la reformulation : a0081fdf reste un faux positif.
- **Autres noms à éléments répétés.** Seul « plus plus » est protégé, pas « C sharp » ni « dot net ».
- Pas de passage de `real_engine_end_to_end` (audio) ni du bench avec `JOT_RESCUE=1`. La reconnaissance vocale et le sauvetage partiel ne sont pas touchés.
- Pas de test manuel dans l'application installée.

## Reproduire

```
cargo test --locked -p jot-core --lib fidelity
JOT_AUDIT_JSONL=<dictations.jsonl d'un export local> cargo test -p jot-core --lib fidelity_audit_replay -- --ignored --nocapture
JOT_LOCAL_E2E=<racine contenant local/models et local/runtime> JOT_REFORM_PACK=fast JOT_REFORM_CASES=<corpus séparés par ;> cargo test --locked -p jot-core --lib reformulation_bench -- --ignored --nocapture
```
