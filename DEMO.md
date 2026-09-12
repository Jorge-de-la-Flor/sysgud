# Demo de Sysgud para video

Esta demo ejecuta el binario real y hace solicitudes HTTP locales. No necesita credenciales: genera un token solo en memoria, desactiva la carga de `.env`, Telegram, Anthropic y el monitor, y guarda el estado en una carpeta temporal dentro de `target`. Al terminar detiene su servidor y elimina sus datos temporales. No modifica `.env` ni envia mensajes externos.

## Preparar y grabar

Desde `C:\Users\tinoc\Dev\sysgud`, con Python 3.10+ y Rust disponibles:

```powershell
cargo build --locked
python scripts/demo-api.py
```

Si Rust no esta en el PATH, esta maquina tiene un entorno local preparado; activalo antes de compilar:

```powershell
. ./target/validation-tools/activate.ps1
```

Abre tu grabador de pantalla sobre esta terminal. El script pausa con **Enter** entre pasos para que puedas explicar cada respuesta. La direccion y el puerto se muestran al iniciar; los tokens no aparecen en pantalla. Para comprobar todo sin pausas:

```powershell
python scripts/demo-api.py --auto
```

## Guion breve

1. **Salud:** `GET /health` devuelve `200` y `status: ok`.
2. **Autenticacion:** consultar incidentes sin token devuelve `401`.
3. **Incidente:** enviar un evento `ERROR` devuelve `201`, con un identificador y estado `pending_approval`.
4. **Control de actor:** aprobar como el actor `999` devuelve `403`.
5. **Aprobacion:** el actor permitido `123` obtiene `202`; el trabajo queda aceptado.
6. **Resultado:** consultar el incidente confirma `executed`.
7. **Repeticion:** aprobar otra vez devuelve `200` con el mismo estado y decision, sin agendar otra ejecucion.
8. **Persistencia:** el script reinicia su servidor con la misma SQLite y comprueba que el incidente y la aprobacion siguen intactos.

La accion de esta demo es **NOTIFY**: `executed` significa que ese flujo termino. No ejecuta comandos ni demuestra una reparacion del sistema operativo. La prueba valida la API local y la persistencia; el modelo de Anthropic, las notificaciones de Telegram y el monitoreo real necesitan una prueba de integracion aparte.

Puedes cancelar con `Ctrl+C`; el script limpia solo su proceso y su carpeta temporal. La demo comparte el enfoque del test existente `scripts/smoke-api.py`, que sigue disponible para comprobaciones automaticas mas amplias.
