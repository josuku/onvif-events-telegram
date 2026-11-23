# Onvif Events Telegram — Agent Rules

## Project Overview

Servicio en Rust que recibe eventos de detección de cámaras ONVIF (movimiento, tamper) y envía notificaciones con snapshots a Telegram. Incluye detección de objetos con YOLO (ONNX Runtime) y descarga de grabaciones.

## Architecture

### Workspace (Cargo workspace)

| Crate                     | Responsabilidad                                                                 |
|---------------------------|---------------------------------------------------------------------------------|
| `app-core`                | Dominio, traits (interfaces), helpers. Sin dependencias de infraestructura.     |
| `onvif`                   | Implementación de clientes ONVIF usando `onvif-rs`.                             |
| `api-camera`              | Clientes de API propietarias de cámaras (Dahua RPC, Hikvision ISAPI, XMeye).   |
| `telegram`                | Bot de Telegram (`teloxide`) y notificador.                                     |
| `repository`              | Persistencia: SQLite (`DbStore`) + caché en memoria (`MemoryRepository`).       |
| `ultralytics-detector`    | Detección de objetos con YOLO11n vía ONNX Runtime.                              |
| `onvif-events-telegram`   | Binario principal. Orquestación, polling, suscripciones, procesador de comandos.|

### Key Design Patterns

- **Trait-based dependency injection**: `app-core/src/traits/` define todas las interfaces (`CommandProcessor`, `Notifier`, `ObjectDetector`, `OnvifCameraClient`, `ApiCameraClient`, `DiscoveryClient`). Las implementaciones concretas están en los crates de infraestructura.
- **Event bus**: `EventBus` con `broadcast::channel` de tokio para desacoplar detección ↔ notificación.
- **Async-first**: todo el runtime es `tokio`. El binario principal usa `tokio::select!` para combinar bot, polling, renovación de suscripciones y shutdown signal.
- **In-memory + SQLite**: `MemoryRepository` mantiene estado en RAM protegido con `Mutex`, sincronizando cambios a `DbStore` (SQLite).

## Coding Guidelines

### Rust Style

- Usa **Rust edition 2024** en los crates nuevos (algunos legacy usan 2021).
- Prefiere `anyhow::Result` para errores en la capa de aplicación y `thiserror` para errores de dominio con variantes tipadas.
- Usa `async_trait` para traits con métodos async.
- Logging con `tracing` (`info!`, `error!`, `warn!`, `debug!`). Nunca `println!`.
- Usa `Arc<dyn Trait>` para compartir implementaciones entre tasks.
- Mantén el patrón: **dominio y traits en `app-core`, implementaciones en crates específicos**.

### Architecture Rules

- **No añadas dependencias de infraestructura a `app-core`**. Este crate debe permanecer puro dominio/traits.
- Los nuevos clientes de API de cámara van en `api-camera`.
- Los nuevos clientes ONVIF van en `onvif`.
- La lógica de Telegram (bot, notificaciones) va en `telegram`.
- Los nuevos comandos del bot:
  1. Añadir la variante al enum `BotCommand` en `telegram/src/telegram_bot.rs`.
  2. Añadir el método al trait `CommandProcessor` en `app-core/src/traits/command_processor.rs`.
  3. Implementar en `onvif-events-telegram/src/app_command_processor.rs`.
  4. Conectar en el match de `process_command` en `telegram_bot.rs`.

### Testing

- No hay tests actualmente. Al añadir funcionalidad nueva, incluir tests unitarios al menos para lógica de dominio pura.
- Para tests que necesiten mocks, usa los traits de `app-core` para crear implementaciones de test.

### Configuration

- Configuración vía `config.yaml` (deserializado con `serde_yaml`).
- La estructura `AppConfig` está en `app-core/src/domain/config.rs`.
- Nuevos parámetros de configuración: añadir al struct correspondiente y al `config.yaml.example`.

### Build & Deploy

- **Build local**: `cargo build` / `cargo run -- config.yaml`
- **Docker**: multi-stage build con `cargo-chef`. Ver `Dockerfile`.
- **Deploy**: `docker-compose.yaml` con `network_mode: host` para WS-Discovery multicast.
- **ONNX Runtime**: libs nativas en `ultralytics-runtime/` (linux, mac, windows). `build.rs` las copia al target.
- **FFMPEG**: necesario para descarga de grabaciones (transcodificación de vídeo).

### Safety & Secrets

- **NUNCA** incluyas tokens de Telegram, contraseñas de cámaras, o IDs de usuario en el código ni en commits.
- El fichero `config.yaml` está en `.gitignore`. Solo se versiona `config.yaml.example`.
- Los ficheros `repo_store.db`, `data/`, y `logs/` no se versionan.

## Idioma

- El código (nombres de variables, funciones, tipos, comentarios en código) debe estar en **inglés**.
- Los mensajes que se envían al usuario de Telegram pueden estar en inglés (actualmente lo están).
- La comunicación con el desarrollador (commits, PRs, issues) puede ser en español o inglés según prefiera.

