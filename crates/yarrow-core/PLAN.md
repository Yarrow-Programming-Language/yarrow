1. Implémenter l'itération `for` avec `list<T>` dans `interpreter/mod.rs` fonction `eval_iter_for` (vers ligne 1575) : ajouter un cas pour `Value::Heap` avec kind `KIND_LIST`, utiliser `runtime::yarrow_list_len` et `runtime::yarrow_list_get` pour itérer, stocker l'index courant et récupérer chaque élément en borrow (owned: false), tester avec list contenant différents types d'éléments

2. Supporter le placeholder vide `{}` pour map et struct dans `interpreter/mod.rs` fonction `eval_expr` (vers ligne 1999) : remplacer l'erreur `Expr::EmptyMapOrStruct` par un appel à `eval_empty_aggregate`, gérer le cas où le type est absent, créer une inférence de type pour les placeholders vides, pour les maps créer un hashmap vide avec type par défaut, pour les structs créer une struct vide avec le type nommé, nécessite un contexte typé (variable declaration) pour fonctionner

3. Supporter les clés et valeurs complexes dans les littéraux hashmap dans `interpreter/mod.rs` fonction `eval_map_lit` (vers ligne 3515) : supprimer les vérifications `ok_or_else` qui retournent "complex map key" et "complex map value", permettre n'importe quel type qui a un `as_bits()` valide, gérer correctement le ownership des clés/valeurs complexes, tester avec des clés struct, enum, pointer, etc., vérifier que le runtime supporte ces types dans `yarrow_map_insert`

4. Supporter les éléments de type array dans les lists dans `interpreter/mod.rs` fonction `eval_list_lit` (vers ligne 3495) : supprimer la vérification qui retourne "array element in list", permettre `Value::Array` comme élément valide, gérer la sérialisation des arrays dans `runtime::yarrow_list_push`, vérifier que le type de la list accepte les arrays, tester avec `list<array<T N>>`

5. Supporter tous les types d'éléments dans les listes dans `interpreter/mod.rs` fonction `eval_std_intrinsic` pour `std.list::push_last` (vers ligne 2370) : étendre la gestion des éléments list pour supporter tous les types, permettre les structs, unions, pointers comme éléments de list, vérifier que `value.value.as_bits()` fonctionne pour tous les types, si un type n'a pas `as_bits()`, utiliser une sérialisation alternative, tester avec `list<Point>`, `list<pointer<i32>>`

6. Améliorer la gestion des struct field access avec des types complexes dans `interpreter/mod.rs` fonction `eval_expr` pour `Expr::Member` (vers ligne 1780) : vérifier tous les cas où `struct_base_addr` retourne une erreur, supporter l'accès aux champs via des pointeurs de structs, supporter l'accès aux champs avec des types unions, tester avec des structs imbriquées et des references

7. Gérer correctement le return de main avec exit code dans `interpreter/mod.rs` fonction `run_entry` (vers ligne 390) : vérifier que `RunResult::Int` est correctement converti en exit code du processus, si main retourne un entier utiliser ce code comme exit code, tester avec `main function do 42 return end` pour obtenir exit code 42

8. Vérifier la portée des `require` dans les fonctions dans `interpreter/mod.rs` fonction `load_requires_stmts` (vers ligne 735) : vérifier que les `StmtKind::Require` dans les corps de fonction ont une portée limitée, s'assurer que les bindings ne sont visibles que dans la fonction, tester avec des requires différents dans différentes fonctions, vérifier que les modules ne sont chargés qu'une seule fois

9. Valider les cas limites de typeof dans `interpreter/mod.rs` fonction `apply_typeof` (vers ligne 2070) : vérifier que `type_kind_code` gère tous les types possibles, tester avec unions, structs, pointers, references, vérifier que typeof retourne le bon code pour chaque type

10. Améliorer la gestion des erreurs dans handle dans `interpreter/mod.rs` fonction `eval_handle` (vers ligne 3276) : vérifier que les erreurs sont correctement propagées, s'assurer que le fallback est correctement poussé sur la stack, tester avec des handlers imbriqués, vérifier que les borrows sont correctement libérés

11. Nettoyer les messages d'erreur E393 dans `interpreter/mod.rs` : parcourir pour tous les `InterpretError::unsupported`, remplacer par des messages spécifiques pour chaque cas, ajouter des codes d'erreur plus précis (E394, E395, etc.)

12. Ajouter des tests pour les nouvelles fonctionnalités : créer des fichiers dans `docs/examples/valid/` pour chaque nouvelle fonctionnalité, tester l'itération sur les lists, tester les placeholders vides, tester les maps avec clés complexes, vérifier que tous les exemples existants fonctionnent toujours
