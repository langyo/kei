# kei Sincronización Upstream

## Descripción

KEI deriva de [Asterinas (星绽)](https://github.com/asterinas/asterinas). Los cambios upstream se absorben periódicamente mediante vendoring a nivel de directorio.

## Sincronización rápida

```bash
just vendor       # Absorber último upstream
just versions     # Mostrar base upstream actual
```

## Frecuencia

- Rutina: cada 3–6 meses
- Corrección urgente: `just vendor-ref <sha>`
