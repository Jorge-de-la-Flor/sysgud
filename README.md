# sysgud 🛡️

> **Agente de runtime autónomo y de baja latencia, escrito en Rust, para debugging proactivo y remediación de procesos directo en la terminal.**

![Language](https://img.shields.io/badge/Language-Rust-orange.svg)
![Runtime](https://img.shields.io/badge/Runtime-Tokio-blue.svg)
![License](https://img.shields.io/badge/License-Proprietary-lightgrey.svg)
![Architecture](https://img.shields.io/badge/Architecture-Single_Crate-purple.svg)

---

## 📌 Overview

**sysgud** saca a los agentes de IA del chatbox tradicional y los mete directo en el entorno de sistema operativo del desarrollador. Corriendo como un daemon autónomo en segundo plano, **sysgud** monitorea streams de salida de procesos (`stdout`/`stderr`) con latencia sub-segundo, mantiene un buffer circular de contexto en memoria, y diagnostica fallas (memory leaks, panics, excepciones no manejadas) de forma independiente al ocurrir.

En vez de requerir cambio manual de contexto o copiar-pegar logs, **sysgud** extrae el límite exacto de la falla, consulta a un motor de razonamiento LLM, y despacha remediaciones automatizadas directo a la sesión de shell nativa.

---

## 🚀 Key Features

* **Zero-Copy Log Ingestion:** streaming asíncrono del proceso supervisado, construido sobre Tokio.
* **Smart Ring Buffer:** snapshot rotativo configurable (`RingBuffer`) que aísla el contexto justo antes de la falla.
* **Sub-second Failure Triggering:** filtros de pattern-matching (`PANIC`, `CRITICAL`, `ERROR`) que evitan llamadas innecesarias al LLM.
* **Structured Action Schema:** salidas JSON del LLM que deserializan a un tipo Rust estricto (`KILL`, `EXECUTE`, `NOTIFY`).
* **Fallback seguro:** si no hay `ANTHROPIC_API_KEY` configurada, o la llamada al agente falla, el daemon nunca se cae — degrada a `NOTIFY`.
* **Native Remediation Runner:** ejecución de comandos del SO vía `tokio::process::Command`, sin overhead de wrappers extra.

---

## 📐 Architecture & Workflow

```text
+-------------------+      +-----------------------+      +-------------------------+
| Target Process    | ---> | Tokio Async Reader    | ---> | Ring Buffer (N líneas)  |
| (stdout / stderr) |      | (mod monitor)         |      | (VecDeque en memoria)   |
+-------------------+      +-----------------------+      +-------------------------+
                                                                       |
                                                           [Anomaly Pattern Detected]
                                                                       v
+-------------------+      +-----------------------+      +-------------------------+
| Native Shell / OS | <--- | Action Runner         | <--- | Agent Engine            |
| (Kill / Exec / UI)|      | (mod actions)         |      | (mod agent, JSON)       |
+-------------------+      +-----------------------+      +-------------------------+
```

La orquestación entre estos tres módulos vive en `src/lib.rs`; `src/main.rs` es intencionalmente delgado y solo arranca el runtime async.

---

## 📂 Project Structure

Un solo crate (`cargo build` sin workspace, sin `path` deps entre proyectos). La separación por dominio (core / monitor / agent / actions) es puramente organizativa: son módulos de Rust, no crates independientes. Cada módulo con submódulos sigue la notación moderna de Rust 2018+ (`nombre.rs` conviviendo con la carpeta `nombre/`, en vez de `nombre/mod.rs`).

```text
sysgud/
├── Cargo.toml              # Un solo paquete, todas las dependencias juntas
├── .env.example            # Variables de entorno soportadas
└── src/
    ├── main.rs             # Binario delgado: solo arranca tokio y llama a sysgud::run()
    ├── lib.rs              # Orquestación: monitor -> agent -> actions
    │
    ├── core.rs             # Tipos, config y errores compartidos (reemplaza a core/mod.rs)
    ├── core/
    │   ├── config.rs       # Config::from_env()
    │   ├── error.rs        # SysgudError
    │   └── types.rs        # ActionType, AgentRequest, AgentAction
    │
    ├── monitor.rs          # Ingesta y ventana de contexto ("Monitor")
    ├── monitor/
    │   ├── buffer.rs       # RingBuffer
    │   └── reader.rs       # spawn() async de stdout/stderr
    │
    ├── agent.rs            # Motor de decisiones ("AgentEngine")
    ├── agent/
    │   ├── client.rs       # AgentClient: llamada a la API de Messages
    │   └── prompt.rs       # System prompt + parsing defensivo del JSON
    │
    ├── actions.rs          # Ejecutor de acciones ("ActionRunner")
    └── actions/
        └── runner.rs       # dispatch por ActionType (reemplaza a runner/mod.rs)
            └── runner/
                ├── kill.rs
                ├── execute_cmd.rs
                └── notify.rs
```

---

## 🛠️ Getting Started

### Prerrequisitos

* Rust 1.75+ (`cargo`, `rustc`) — instalado vía [rustup](https://rustup.rs/) recomendado.
* Python 3.x (opcional, solo para el script de demo que simula un crash).
* Una `ANTHROPIC_API_KEY` si quieres diagnóstico en vivo del LLM (opcional: sin ella, el agente responde siempre `NOTIFY`).

### Instalación y compilación

```bash
# Clonar el repositorio
git clone https://github.com/tu-usuario/sysgud.git
cd sysgud

# Copiar y completar las variables de entorno (opcional)
cp .env.example .env

# Compilar en modo debug
cargo build

# Correr el daemon (usa el script de demo por defecto)
cargo run

# Compilar el binario optimizado
cargo build --release
./target/release/sysgud
```

### Tests

```bash
cargo test
```

---

## ⚙️ Configuración

Todas las variables son opcionales; ver [`.env.example`](./.env.example) para el detalle completo. Las más relevantes:

| Variable               | Default          | Descripción                                              |
|------------------------|------------------|-----------------------------------------------------------|
| `ANTHROPIC_API_KEY`    | *(vacío)*        | Si falta, el agente degrada a `NOTIFY` sin fallar.        |
| `SYSGUD_MODEL`         | `claude-sonnet-5`| Modelo invocado en la API de Messages.                    |
| `SYSGUD_CONTEXT_LINES` | `12`             | Tamaño del buffer circular de contexto.                   |
| `SYSGUD_TARGET_CMD`    | `python3`        | Binario del proceso supervisado.                          |
| `SYSGUD_TARGET_ARGS`   | *(demo OOM)*     | Argumentos del proceso supervisado, separados por espacio.|

---

## 📋 Remediation Action Schema

El agente devuelve JSON estricto que deserializa directo a `sysgud::core::AgentAction`:

| Action Type | Descripción                                                        | Ejemplo de payload |
|-------------|---------------------------------------------------------------------|---------------------|
| `NOTIFY`    | Reporta el diagnóstico en consola sin tocar el proceso supervisado. | `{"action_type": "NOTIFY", "command": null, "diagnosis": "OOM in worker thread"}` |
| `KILL`      | Termina el proceso supervisado (usa el PID capturado por el monitor).| `{"action_type": "KILL", "command": null, "diagnosis": "Deadlock detected"}` |
| `EXECUTE`   | Corre un comando de shell de remediación.                           | `{"action_type": "EXECUTE", "command": "rm -f /tmp/lock", "diagnosis": "Stale lock file"}` |

---

## 🔀 ¿Y si esto crece y necesito separar en crates?

Si en algún momento un módulo necesita compilarse independiente (por ejemplo, publicar `core` como librería standalone, o reusar `monitor` en otro binario sin arrastrar `reqwest`), el camino es:

1. `cargo new --lib crates/sysgud-<modulo>` y mover el contenido de `src/<modulo>.rs` + `src/<modulo>/` ahí adentro.
2. Cambiar `use crate::<modulo>::...` por `use sysgud_<modulo>::...` en los lugares que lo consuman.
3. Agregar la entrada en `[workspace]` y la dependencia `path = "crates/sysgud-<modulo>"`.

Es un refactor mecánico y acotado — no hace falta pagar ese costo de antemano.

---

## 📄 Licencia

Por ahora este es código cerrado — todavía no está decidida la licencia definitiva. El badge de licencia se actualizará cuando eso se resuelva.
