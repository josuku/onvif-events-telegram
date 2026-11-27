# Project Memory — Onvif Events Telegram

> Este fichero sirve como memoria persistente del proyecto. Actualízalo cuando se tomen decisiones de diseño, se resuelvan bugs complicados, o se descubra información relevante.

---

## Project Facts

- **Nombre**: onvif-events-telegram
- **Lenguaje**: Rust (edition 2024 / 2021 legacy en algunos crates)
- **Async runtime**: tokio (features = full)
- **Bot framework**: teloxide
- **ONVIF**: onvif-rs (cliente WS)
- **Object detection**: YOLO11n vía ONNX Runtime 1.28.0
- **DB**: SQLite vía rusqlite (crate `repository`)
- **Serialización**: serde + serde_yaml (config), serde_json (domain)
- **Error handling**: anyhow (app) + thiserror (dominio)
- **Logging**: tracing + tracing-subscriber (stdout + rolling file)
- **Deploy**: Docker (multi-stage con cargo-chef), docker-compose con network_mode: host

---

## Crate Dependency Graph

```
onvif-events-telegram (binary)
├── app-core          (domain + traits)
├── app-services      (CameraService, ConfigService, MediaService, AuthService)
│   ├── app-core
│   ├── api-camera
│   ├── onvif
│   └── repository
├── api-http          (Axum REST API + Swagger UI + JWT Auth)
│   ├── app-core
│   └── app-services
├── api-camera        (Dahua, Hikvision, XMeye clients)
│   └── app-core
├── onvif             (onvif-rs camera + discovery clients)
│   └── app-core
├── telegram          (teloxide bot + notifier)
│   └── app-core
│   └── repository
├── repository        (SQLite + memory cache)
│   └── app-core
│   └── onvif
└── ultralytics-detector (YOLO ONNX)
    └── app-core
```

---

## Key Types & Traits

### Traits (app-core/src/traits/)

| Trait               | Fichero                  | Propósito                                          |
|---------------------|--------------------------|----------------------------------------------------|
| `CommandProcessor`  | `command_processor.rs`   | Procesamiento de todos los comandos del bot         |
| `Notifier`          | `notifier.rs`            | Enviar mensajes/fotos/vídeos a Telegram             |
| `OnvifCameraClient` | `onvif_camera_client.rs` | Interacción ONVIF: snapshots, eventos, suscripción  |
| `ApiCameraClient`   | `api_camera_client.rs`   | API propietaria: grabaciones, zoom óptico           |
| `ObjectDetector`    | `object_detector.rs`     | Detección de objetos en imagen                      |
| `DiscoveryClient`   | `discovery_client.rs`    | Descubrimiento ONVIF de cámaras en red             |

### Domain (app-core/src/domain/)

| Tipo              | Fichero               | Descripción                                    |
|-------------------|-----------------------|------------------------------------------------|
| `AppConfig`       | `config.rs`           | Configuración raíz (telegram + bot_config)     |
| `BotConfig`       | `config.rs`           | Config de polling, detector, errores, etc.     |
| `CameraData`      | `camera.rs`           | Estado completo de una cámara                  |
| `CameraEvent`     | `camera.rs`           | Evento de detección con snapshot y objetos      |
| `RecordingSummary`| `camera.rs`           | Resumen de grabaciones (archivos, MB, intervalo)|
| `EventBus`        | `event_bus.rs`        | Pub/sub con broadcast::channel                 |
| `Object`          | `object.rs`           | Objeto detectado (clase, confianza, bbox)       |
| `ErrorMessage`    | `error_message.rs`    | Error de sincronización con cámara             |

### Type Aliases (app-core/src/lib.rs)

- `CameraId = i64`
- `SubscriptionId = i64`
- `ChatId = i64`
- `MessageId = i32`

---

## Bot Commands (Telegram)

### Cámaras
- `/getcameras [extended]` — listar cámaras
- `/addcamera uri [user] [pass]` — añadir cámara manual
- `/deletecamera camera_id` — eliminar cámara
- `/setcameraname camera_id name` — renombrar
- `/setcredentials camera_id user pass` — actualizar credenciales
- `/fixsnapshot camera_id` — arreglar URI de snapshot
- `/setzoom camera_id 1-100` — zoom óptico

### Eventos
- `/subscribe camera_id` — suscribirse a detecciones
- `/unsubscribe camera_id` — desuscribirse
- `/getsnapshot [camera_id]` — snapshot manual
- `/detect [camera_id]` — detección manual puntual de objetos en una o todas las cámaras
- `/manualdetection enable [interval] [types...]` — escaneo visual en background con detección YOLO secuencial
- `/manualdetection disable` — desactiva el escaneo en background
- `/manualdetection [status]` — consulta estado del escaneo en background
- `/dailyreport true|false` — informe diario

### Grabaciones
- `/getrecordingsummary camera_id [days]` — resumen rápido de grabaciones (rango fechas, archivos, tamaño)
- `/getrecordinglist camera_id date [clip_seconds]` — lista de archivos con botones de descarga (admite ISO8601 o keywords: `today`, `yesterday`, `last monday`, `30m ago`, etc.)
- `/downloadrecording camera_id date [clip_seconds]` — descarga clip en fecha (admite ISO8601 o keywords: `today`, `yesterday`, `last monday`, `30m ago`, etc.)

### Configuración
- `/getconfig` — ver config actual
- `/resetconfig` — restaurar defaults
- `/configpollingtime seconds` — intervalo de polling
- `/configbetweentime seconds` — tiempo mínimo entre notificaciones
- `/configsenderrors true|false` — recibir errores de sync
- `/configautorenewal true|false` — renovación automática ONVIF
- `/configrecordingclip seconds` — duración clip de grabación
- `/configdetectorenable true|false` — activar detector YOLO
- `/configdetectorminconfidence 0-1` — confianza mínima
- `/configdetectortypes person,cat,...` — tipos a detectar
- `/configsnapshot camera_id api|rtsp` — método de snapshot
- `/configmultichannel camera_id true|false` — detección multi-canal

---

## Main Loop Flow

```
main()
├── init_logging()
├── read_config(config.yaml)
├── create EventBus, DbStore, MemoryRepository
├── create TelegramNotifier, UltralyticsDetector, AppCommandProcessor
├── create TelegramBot
└── tokio::select!
    ├── start_bot()                    → teloxide dispatcher
    ├── start_polling()                → loop: check_cameras + daily_report
    ├── renew_subscriptions()          → loop: renew ONVIF subs cada 25 min
    ├── start_manual_detection_loop()  → loop: escaneo secuencial en background con YOLO
    └── shutdown_signal()              → SIGTERM/Ctrl+C → close subs
```

---

## Known TODOs (from README)

- [x] Comandos: getrecordingsummary, getrecordinglist and downloadrecording (ISO8601 + clip)
- [x] Detección manual (por cámara o batch)
- [ ] Obtener servicios ONVIF disponibles
- [ ] Implementación alternativa de ONVIF (oxvif)
- [ ] Implementación alternativa de detection (ort directamente)
- [ ] Comando reboot camera (indicar errores de polling)
- [ ] Comando getvideo (camera + duración)
- [ ] Guardar detecciones en PostgreSQL + fotos en carpeta (con cleanup)
- [ ] Frontend web para buscar detecciones por fecha, cámara u objeto

---

## Decisions Log

> Añade aquí las decisiones de diseño importantes y su justificación.

| Fecha | Decisión | Contexto |
|-------|----------|----------|
| —     | trait-based DI con `async_trait` | Permite testear con mocks y desacoplar capas |
| —     | `MemoryRepository` + `DbStore` | Rendimiento (RAM) + persistencia (SQLite). Sin ORM. |
| —     | `broadcast::channel` para EventBus | Múltiples consumidores, un productor |
| —     | YOLO11n ONNX en vez de llamada a servicio | Funciona offline, latencia mínima |
| —     | `cargo-chef` en Docker | Cacheo de dependencias, rebuild rápido |
| —     | Fallback sin `Dirs` en Dahua RPC | Error `Component error: interface not found!` al forzar `/mnt/sd` en cámaras sin SD montada |
| —     | Botones inline para descarga (`getrecordinglist`) | Permite descargar clips directamente clicando en Telegram usando callbacks |
| —     | Escaneo secuencial para `manualdetection` | Evita saturar la CPU/GPU al procesar varias cámaras a la vez con YOLO |
| —     | `network_mode: host` en Docker | Necesario para WS-Discovery multicast UDP |
| 2026-11-05 | Inyección de `ObjectDetector` en `AppCommandProcessor` | Permite que `/detect` invoque el detector YOLO directamente tanto para una cámara como en batch |
| 2026-11-05 | Parser flexible de ISO8601 para comandos de grabaciones | Soporta RFC3339 (`2026-11-05T15:30:00Z`, con offset), formato naive (`YYYY-MM-DDTHH:MM:SS`) y fecha sola (`YYYY-MM-DD`) |
| 2026-11-05 | Integración en `ApiCameraClient` de `get_recording_summary` y `get_recording_list` | Implementados en todos los clientes (`DahuaRpcApiCameraClient`, `HikvisionIsapiApiCameraClient`, `DvrIpXmeyeApiCameraClient`) |
| 2026-11-16 | Paginación de grabaciones en Telegram (`/getrecordinglist`) | Mensajes de 10 en 10 elementos con botones de paginación (`⏮️`, `◀️`, `▶️`, `⏭️`) para evitar exceder los 4096 caracteres de Telegram. |
| 2026-11-16 | Manejo de zonas horarias en listas de grabaciones | Parseo de horas de cámara locales a UTC al generar callbacks de descarga. |
| 2026-11-16 | Actualización granular de botones inline | Al pulsar una descarga, solo se modifica el botón individual (`⏳ Downloading...` / `✅ Downloaded` / `❌ Error`), preservando el resto de botones y la paginación. |
| 2026-11-16 | Corrección de descarga en frontera entre archivos consecutivos | Uso de intervalo semiabierto `[begin, end)` en Dahua, Hikvision ISAPI y XMeye para evitar que la hora coincidente con el fin del archivo anterior descargue el archivo erróneo. |
| 2026-11-16 | Descarga completa de archivo seleccionado en lista | Se pasa la duración real (`end - begin`) en el callback para que no se recorte al valor por defecto de 30s. |
| 2026-11-21 | Fase 1 & 2: Capa de servicios (`app-services`) y REST API (`api-http`) | Extracción de lógica de negocio a servicios (`CameraService`, `ConfigService`, `MediaService`) y creación de API HTTP con Axum y OpenAPI/Swagger UI. |
| 2026-11-21 | Fase 3: Autenticación JWT y Seguridad API | Mecanismo de autenticación JWT con credenciales en `config.yaml` (`http.auth`), endpoint público de login `/api/v1/auth/login` (emite token Bearer y cookie `Set-Cookie: token=...`), logout `/api/v1/auth/logout`, `/api/v1/auth/me`. Middleware de protección universal (soporta header `Authorization: Bearer`, cookie y query param `?token=...`) protegiendo todas las rutas de API y Swagger UI (`/swagger-ui`, `/api-docs`). |
| 2026-11-23 | Fase 4: Frontend Web con Modern Web Guidance | Dashboard SPA servido directamente por Axum embebido con `rust-embed` sin dependencias externas ni build steps pesados. Implementado con estándares web modernos: native HTML5 `<dialog>`, CSS Grid, Container Queries (`@container`), CSS custom properties, diseño responsive adaptado a móvil/tablet con barra de navegación inferior nativa y controles táctiles de 44px, modo light/dark, ES Modules, Canvas 2D overlay para bounding boxes YOLO, endpoints de grabaciones (`/api/v1/cameras/{id}/recordings`, `/summary`, `/download`) y ajuste de sensibilidades YOLO/detección en tiempo real. |

---

## Lessons Learned

> Apunta aquí problemas encontrados y cómo se resolvieron.

| Problema | Solución |
|----------|----------|
| Snapshot URI con credenciales inválidas en cámaras chinas | Usar ONVIF Device Manager para crear nuevo usuario; reemplazar en URI |
| ONVIF PullMessages falla intermitentemente | Reintentos con backoff exponencial (3 intentos directos → luego cada 30 ciclos) |
| Snapshot API falla en algunas cámaras | Fallback automático a RTSP snapshot |

---

## Camera Protocol Support

| Protocolo        | Crate          | Funciones                                    |
|------------------|----------------|----------------------------------------------|
| ONVIF WS         | `onvif`        | Discovery, eventos, snapshots, suscripciones |
| Dahua RPC        | `api-camera`   | Grabaciones, zoom óptico                     |
| Hikvision ISAPI  | `api-camera`   | Grabaciones, zoom óptico                     |
| XMeye DVRIP      | `api-camera`   | Grabaciones (parcial)                        |

---

## File Locations Quick Reference

```
config.yaml              → configuración runtime (no versionado)
config.yaml.example      → plantilla de configuración
repo_store.db            → SQLite database (no versionado)
models/yolo11n.onnx      → modelo YOLO
logs/                    → rolling logs diarios
ultralytics-runtime/     → ONNX runtime libs (linux/mac/windows)
```
