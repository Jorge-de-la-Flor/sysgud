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

## Lo que muestra la demo local

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

## Prueba con contenedores

Requisito: **Docker Desktop iniciado en modo de contenedores Linux**, con Docker Compose disponible. Ejecuta desde la raiz del repositorio.

Para una prueba automatica independiente, construye y ejecuta la API y su cliente de prueba:

```powershell
docker compose -f compose.demo.yaml up --build --abort-on-container-exit --exit-code-from demo
docker compose -f compose.demo.yaml down
```

La demo es efimera: usa una red interna sin publicar puertos, un token publico exclusivo de prueba y datos temporales. No usa las credenciales de `.env`, Telegram, el modelo ni el monitor. El codigo de salida corresponde a la prueba del servicio `demo`.

Para ejecutar el servicio real, primero completa la vinculacion de Telegram indicada abajo y detén cualquier instancia local que use el mismo bot o el puerto 3000. Despues:

```powershell
docker compose up --build -d
docker compose ps
docker compose logs --tail 50 sysgud
```

Compose entrega las variables de `.env` al contenedor al ejecutarlo; las credenciales no se incluyen en la imagen. La API se publica en `http://127.0.0.1:3000` y puedes probarla desde el host con `python scripts/demo-telegram.py`. Este despliegue deja el monitor desactivado.

Para detenerlo:

```powershell
docker compose down
```

El volumen `sysgud-data` conserva el historial entre arranques. Usa este despliegue o `scripts/start.ps1` para el servicio real; evita dos instancias consultando el mismo bot.

## Telegram real junto a la API

Prepara esta parte antes de grabar. Requiere conexion a Internet, un token de bot valido en `.env` y vincular tu cuenta privada. El token del bot y el token Bearer de la API son credenciales distintas; no los muestres en el video.

Con el servidor detenido, inicia la vinculacion y pulsa **Iniciar / Start** en el enlace que se abre:

```powershell
python scripts/connect-telegram.py --open --wait 900
```

Continua cuando el script confirme `paired: true`. La vinculacion registra el ID autorizado, activa Telegram y deja el monitor desactivado para esta demostracion. No ejecutes el vinculador mientras el servidor esta consultando Telegram.

En una terminal, valida la configuracion e inicia el servicio:

```powershell
./scripts/start.ps1 -Check
./scripts/start.ps1
```

Si el servicio ya esta iniciado, conserva esa instancia. En otra terminal, ejecuta:

```powershell
python scripts/demo-telegram.py
```

El script comprueba la salud y el acceso autenticado a la API local, crea un incidente con origen `hackathon-video` y espera hasta 120 segundos por una decision hecha desde Telegram. Puedes ampliar la espera con `--timeout 300`. Deja visibles esta terminal y el chat privado del bot:

1. En Telegram, envia `/status` para consultar los incidentes pendientes.
2. Revisa el aviso o envia `/incident ID`, sustituyendo `ID` por el identificador que muestra la terminal.
3. Comprueba que la accion propuesta sea `NOTIFY` y envia el `/approve ID` de ese incidente.
4. Consulta `/incident ID` para ver el resultado. La terminal comprueba el estado guardado por la API; la respuesta inicial de Telegram puede mostrar `Executing` mientras termina.

Para demostrar rechazo, crea otro incidente y usa `/reject ID`. Cada decision corresponde a un incidente concreto. Estos incidentes se conservan en la base local del servicio; agotar la espera del script no los aprueba ni elimina.

Sin `ANTHROPIC_API_KEY`, el diagnostico usa **NOTIFY de respaldo**. La aprobacion puede finalizar como `executed`, pero no demuestra una llamada al modelo ni una reparacion real. Si la propuesta es diferente de `NOTIFY`, revisala antes de aprobar y usa la demo local para grabar el flujo previsto.

## Guion de 90 segundos

Ensaya una vez y deja Telegram vinculado y el servicio iniciado antes de grabar. Ejecuta la demo local en una segunda terminal; usa **Enter** para avanzar. Al terminar, ejecuta la demo de Telegram en esa misma terminal.

- **0–10 s:** "Sysgud convierte eventos de error en incidentes y solicita una decision humana antes de completar una accion." Muestra el inicio y `GET /health`.
- **10–25 s:** "La API exige autenticacion y valida quien puede aprobar." Muestra `401` sin token, la creacion del incidente y `403` para el actor no autorizado.
- **25–40 s:** "Un usuario autorizado aprueba este incidente concreto." Muestra `202` y luego `executed`; aclara que la accion de la prueba es `NOTIFY`.
- **40–55 s:** "Repetir la aprobacion conserva la decision, y el reinicio conserva el historial." Avanza por repeticion y persistencia hasta `DEMO OK`.
- **55–80 s:** Ejecuta `python scripts/demo-telegram.py`. "Ahora uso Telegram contra el servicio local." Muestra el incidente, envia `/approve ID` y muestra el resultado consultado por la API. Reserva unos segundos para la red.
- **80–90 s:** "La integracion conserva la identidad del aprobador y el resultado. Esta demostracion usa NOTIFY; el diagnostico del modelo requiere su credencial y la reparacion requiere una accion configurada y verificada."

Si Telegram no responde durante el ensayo, comprueba que la vinculacion termino, que usas el chat privado autorizado y que solo hay una instancia del bot. La demo local sigue disponible para mostrar el flujo HTTP, sin atribuirle una prueba real de Telegram.
