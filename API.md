# API sysgud v1

Base predeterminada: `http://127.0.0.1:3000`. Es una API local sin CORS. Todas las rutas `/api/v1/*` requieren exactamente una cabecera `Authorization: Bearer <SYSGUD_BOT_API_TOKEN>`. El token de Telegram no sirve para esta cabecera.

Contrato completo: [openapi.json](crates/sysgud-api/openapi.json). Las respuestas de error son JSON con `error`, excepto `/health`, que usa `status`. Se devuelven `Cache-Control: no-store` y `X-Content-Type-Options: nosniff`.

## Flujo

1. `GET /health`: 200 si el servicio admite trabajo y su almacenamiento está disponible; 503 al detenerse o tras fallar una escritura. No verifica credenciales externas ni que el proceso supervisado siga vivo.
2. `POST /api/v1/events` con `{"source":"worker","message":"ERROR: disk full"}`: 201 y un incidente si hay disparador; 202 y `{"status":"buffered"}` en caso contrario. `log_line` es alias de `message`; usar ambos es inválido. No se aceptan acciones, comandos, PIDs ni contexto arbitrario como campos adicionales.
3. `GET /api/v1/incidents?status=pending_approval&limit=50&offset=0`: array paginado. `limit` está entre 1 y 100; `offset` entre 0 y 10000. El orden es por UUID. Los cambios de retención pueden mover elementos entre páginas.
4. `GET /api/v1/incidents/UUID`: diagnóstico, logs redactados, `proposed_action`, invocación fija `execution`, estado y datos de decisión.
5. `POST /api/v1/incidents/UUID/approve` o `/reject` con `{"actor_id":123456789}`. Solo se acepta ese campo y el actor debe pertenecer a la lista permitida.

Antes de aprobar EXECUTE revisa `execution.program` y todos sus `args`. El cliente HTTP no puede cambiarlos. KILL solo admite el handle del proceso supervisado; nunca un PID aportado por un cliente.

La primera aprobación devuelve 202 con estado `executing`. Consulta el detalle hasta `executed` o `failed`; no interpretes 202 como éxito del comando. Repetir la misma decisión y actor devuelve 200 sin repetir el efecto, incluso después de reiniciar. Otro actor o decisión devuelve 409. El rechazo devuelve 200 y `rejected`.

Una ejecución interrumpida al cerrar el daemon se recupera como `failed` con resultado desconocido. No se reproduce automáticamente. Una decisión que ya salió de la retención devuelve 404.

## Códigos de error

- 400: campos vacíos, rangos, UUID o query inválidos.
- 401: token ausente, incorrecto o cabeceras duplicadas.
- 403: actor fuera de la lista.
- 404: incidente inexistente o ya retirado.
- 408: solicitud supera 40 segundos.
- 409: decisión incompatible con la ya almacenada.
- 413: cuerpo mayor de 16 KiB.
- 415: contenido sin `application/json`.
- 422: JSON no compatible, propiedades adicionales o aprobación bloqueada por política/caducidad.
- 429: concurrencia, frecuencia o capacidad agotada. No repitas indefinidamente: respeta `Retry-After`, revisa pendientes y contexto.
- 503: persistencia indisponible; las acciones quedan bloqueadas.

`source` admite hasta 256 bytes UTF-8 sin caracteres de control y `message` hasta 8192 bytes. Los límites de bytes prevalecen sobre las restricciones de caracteres del esquema.

## Ejemplo PowerShell

Con el token interno ya exportado a la terminal, sin escribirlo en el historial:

```powershell
$headers = @{ Authorization = "Bearer $env:SYSGUD_BOT_API_TOKEN" }
$incident = Invoke-RestMethod http://127.0.0.1:3000/api/v1/events -Method Post -Headers $headers -ContentType application/json -Body '{"source":"demo","message":"ERROR: fallo de prueba"}'
Invoke-RestMethod "http://127.0.0.1:3000/api/v1/incidents/$($incident.id)" -Headers $headers
# Rechazo de ejemplo; sustituye por tu ID permitido.
$decision = @{ actor_id = 123456789 } | ConvertTo-Json
Invoke-RestMethod "http://127.0.0.1:3000/api/v1/incidents/$($incident.id)/reject" -Method Post -Headers $headers -ContentType application/json -Body $decision
```

El bot obtiene `actor_id` del remitente autenticado por Telegram. La API confía en el poseedor del token interno para afirmar esa identidad; no es un sistema de autenticación independiente por usuario.

Los eventos no son idempotentes: si se pierde la respuesta de creación, consultar incidentes antes de reenviar evita duplicados de diagnóstico. Las decisiones sí son idempotentes por incidente, actor y decisión.
