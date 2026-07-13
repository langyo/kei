# kei Synchronisation Amont

## Aperçu

KEI est dérivé d'[Asterinas (星绽)](https://github.com/asterinas/asterinas). Les changements amont sont absorbés périodiquement par vendoring au niveau répertoire, plutôt que par `git merge`.

## Synchronisation rapide

```bash
just vendor       # Absorber le dernier amont
just versions     # Afficher la base amont actuelle
```

## Fréquence

- Routine : tous les 3–6 mois
- Correctif urgent : `just vendor-ref <sha>`
