# sysgud

Agente local de diagnóstico de procesos con API REST, almacenamiento SQLite y bot de Telegram. Los logs generan propuestas; KILL y EXECUTE requieren la aprobación de un usuario permitido. Las decisiones se reservan en disco antes de actuar y no se reejecutan después de un reinicio.

Esta versión integra `origin/telegram` (`3ee537c`), la API local anterior y el parche de protección del runtime. El historial y los documentos `openspec/changes/` de upstream se conservan como antecedentes; el contrato vigente está en [API.md](API.md).

## Arranque en Windows

Desde la carpeta del proyecto:

```powershell
./scripts/start.ps1 -Check
./scripts/start.ps1
```

El script usa Rust del PATH o, en esta computadora, las herramientas locales de `target/validation-tools`. En otros equipos instala Rust estable. Con Rust disponible, los equivalentes son `cargo run -- --check` y `cargo run`.

Si aún no tienes `.env`, copia `.env.example` una sola vez. Si ya existe, consérvalo. Completa:

- `SYSGUD_BOT_API_TOKEN`: secreto interno aleatorio de 32–256 caracteres ASCII sin espacios. Puedes generar 64 caracteres con `[Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32))`.
- `SYSGUD_ALLOWED_TELEGRAM_USER_IDS`: IDs numéricos de quienes pueden decidir; vacío bloquea todas las decisiones.
- Para iniciar solo la API, establece `SYSGUD_MONITOR_ENABLED=false`.

La API escucha por defecto en `http://127.0.0.1:3000`. `SYSGUD_API_HOST=0.0.0.0` permite escuchar dentro de un contenedor; Compose publica el puerto solo en la interfaz local del equipo. `--check` valida la configuración y abre la base, pero no inicia procesos supervisados ni conexiones a Telegram/Anthropic. El arranque y las demos no modifican tu `.env`.

## Telegram

Tu token de BotFather va en `TELEGRAM_BOT_TOKEN` dentro de `.env`. Es diferente de `SYSGUD_BOT_API_TOKEN`.

Con el servidor detenido, vincula tu cuenta desde el enlace privado:

```powershell
python scripts/connect-telegram.py --open --wait 900
```

Pulsa **Iniciar / Start** en Telegram. El script valida el token, vincula el ID del remitente del enlace y configura el menú del bot. Actualiza solo las listas de usuarios, el chat de avisos y los interruptores de Telegram y monitor en `.env`; conserva las demás claves. El enlace caduca a los 15 minutos. Un webhook existente impide la vinculación y no se elimina. El monitor queda desactivado para preparar la demo. Después inicia `./scripts/start.ps1`.

En Windows, `./scripts/start-telegram.ps1` vincula la cuenta y arranca el servicio en una sola ejecución. No lo ejecutes a la vez que otro servidor del mismo bot.

Para grabar el flujo, usa `python scripts/demo-api.py` o, con Telegram vinculado y el servicio iniciado, `python scripts/demo-telegram.py`. El [guion de video](DEMO.md) incluye ambas demostraciones y sus requisitos.

## Contenedores

Con Docker y Compose disponibles, la prueba completa sin credenciales externas se ejecuta con:

```powershell
docker compose -f compose.demo.yaml up --build --abort-on-container-exit --exit-code-from demo
```

Levanta una API aislada y un contenedor de pruebas; termina con código cero si la demo pasa. Para iniciar el servicio con tu `.env` ya configurado, usa `docker compose up --build -d`. Consulta los pasos y la persistencia en [DEMO.md](DEMO.md). No ejecutes la instancia local y el contenedor con el mismo bot simultáneamente.

Para habilitar el bot, añade usuarios permitidos y establece `SYSGUD_TELEGRAM_ENABLED=true`. `TELEGRAM_ALLOWLIST` se admite como alias de la rama original; si ambas listas tienen valores deben coincidir. `TELEGRAM_CHAT_ID` es opcional y debe ser el chat privado de uno de esos usuarios para recibir avisos de nuevos incidentes.

Abre una conversación privada con tu bot y usa:

- `/status` o `/incidents`: hasta diez incidentes pendientes.
- `/incident UUID`: detalle de la propuesta y la invocación configurada.
- `/approve UUID`: aprobar exactamente ese incidente.
- `/reject UUID`: rechazarlo.
- `/help`: ayuda.

Se conservan los sufijos como `/status@nombre_bot`. Los comandos globales `/approve` y `/reject` sin ID ya no deciden sobre una propuesta mutable. Usuarios ajenos a la lista, mensajes sin remitente y grupos se ignoran. El actor procede de `message.from.id`, nunca del texto.

Las respuestas reflejan el estado devuelto por la API: `executing` no significa ejecución completada. Un fallo de envío no se reporta como entrega exitosa. Los avisos son de mejor esfuerzo; ante desconexiones o saturación consulta `/status` y la API. No hay un segundo ejecutor exclusivo del bot.

El polling usa la [Bot API oficial](https://core.telegram.org/bots/api#getupdates). Requiere que no haya otro polling o webhook activo para ese mismo bot. No se desactiva automáticamente una integración existente.

## Monitor y política de acciones

Establece `SYSGUD_MONITOR_ENABLED=true`, `SYSGUD_TARGET_CMD` y `SYSGUD_TARGET_ARGS`. Los argumentos aceptan un array JSON, incluyendo `[]`. También se conserva la sintaxis simple de comillas sin expansión de variables. Una configuración inválida falla al iniciar.

- `NOTIFY`: registra el diagnóstico y, si configuraste avisos, lo comunica. Aprobarlo solo registra la decisión.
- `KILL`: solo puede terminar el hijo que posee el monitor. Una solicitud HTTP nunca elige un PID. Tras reiniciar, los handles antiguos no se recuperan y esos KILL quedan bloqueados.
- `EXECUTE`: el campo `command` del agente debe ser un ID incluido en `SYSGUD_COMMANDS_JSON`. Cada ID apunta a un ejecutable absoluto existente y argumentos literales. La API muestra la invocación completa en `execution`. Cambiar esa definición invalida las aprobaciones pendientes correspondientes. Sin configuración, EXECUTE está bloqueado.

Los comandos no pasan por un shell construido con texto del modelo. Se cierran stdin/stdout/stderr, se filtran variables sensibles y se limita cada ejecución a 30 segundos. Esto no es un sandbox de sistema operativo: el ejecutable autorizado tiene los permisos de sysgud y podría crear descendientes. Usa una cuenta con permisos limitados y configura solo programas que conozcas.

Las credenciales conocidas del entorno y patrones habituales se ocultan antes del almacenamiento y del LLM. La redacción no garantiza reconocer todos los secretos posibles: evita supervisar logs con datos que no deban salir del equipo. Sin `ANTHROPIC_API_KEY` no se llama al modelo.

## Límites del workspace

- `sysgud-core`: tipos y contrato de incidentes. Solo depende directamente de serde, chrono y uuid.
- `sysgud-runtime`: monitor, agente, redacción, SQLite y ejecución autorizada. No depende de los transportes.
- `sysgud-api`: HTTP, autenticación, validación y traducción de errores. Delega las decisiones al runtime.
- `sysgud-telegram`: transporte de Telegram y cliente de la API. No puede ejecutar procesos; sus dependencias normales solo acceden al dominio.
- `sysgud`: configuración y ciclo de vida. Compone los cuatro crates.

`scripts/check-boundaries.py` comprueba las dependencias entre crates en CI. Las dependencias de pruebas del bot permiten verificar su integración con la API sin contactar Telegram.

Límites operativos: líneas de 8 KiB, contexto de 32 KiB por origen, 256 orígenes, cola de 256 líneas por monitor, 4 análisis simultáneos, 30 análisis por minuto, 60 eventos por segundo, 4 ejecuciones simultáneas y 32 solicitudes HTTP activas. El cuerpo HTTP se limita a 16 KiB y cada solicitud a 40 segundos. La lista usa páginas de hasta 100 registros.

SQLite retiene 1000 incidentes por defecto, configurable hasta 10000. Al llenarse elimina el incidente terminal más antiguo; nunca elimina pendientes o ejecuciones en curso para aceptar otro. Si solo hay pendientes devuelve capacidad agotada: resuélvelos antes de seguir. La caducidad de aprobación es de 15 minutos por defecto; rechazar sigue disponible.

Una sola instancia puede usar cada base. Un fallo de escritura bloquea nuevas acciones y hace fallar `/health`. Tras una interrupción durante EXECUTE/KILL, el resultado se marca como desconocido y no se reintenta. Las notificaciones de Telegram no tienen una cola durable.

## Verificación

```powershell
./scripts/verify.ps1
```

Ejecuta formato, pruebas del workspace, Clippy con advertencias como errores y comprobación de límites entre crates. Las pruebas cubren autenticación, actores, propuestas inmutables, reintentos concurrentes, errores de ejecución, persistencia tras reiniciar, recuperación de resultados desconocidos, retención, redacción de secretos y Telegram contra servidores HTTP de prueba.

Para reproducir el flujo HTTP con una base temporal y sin credenciales externas: `python scripts/smoke-api.py`, después de `cargo build`.

El archivo [OpenAPI](crates/sysgud-api/openapi.json) describe las rutas y también se sirve autenticado en `/api/v1/openapi.json`. Las pruebas locales no sustituyen una prueba real con tu bot, tus usuarios y tu modelo. El CI preparado cubre Windows y Linux; sus ejecuciones remotas dependen de publicar los cambios.

No se ha definido una licencia pública para este proyecto.
