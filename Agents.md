# Guía de trabajo para agentes de IA

Este archivo da contexto operativo a ChatGPT, Codex y otros agentes que
modifiquen Agora. La documentación para personas está en `README.md` y las
decisiones de producto en `decisiones.md`.

## Alcance invariable

Agora es un backend y worker en Rust para un único grupo cerrado de WhatsApp.
No hay aplicación web. Los únicos endpoints públicos son el webhook, health,
readiness y los tres avisos legales.

La versión 1:

- usa exclusivamente las APIs oficiales WhatsApp Cloud y Groups;
- acepta texto, DOC, DOCX, PDF, XLS y XLSX en español;
- responde sólo cuando un participante autorizado invoca `@agora`;
- aísla siempre datos por `WHATSAPP_GROUP_ID`;
- usa OpenAI para embeddings y generación;
- conserva archivos originales como binarios en PostgreSQL;
- se despliega en `oracle` mediante la imagen ARM64 publicada en GHCR.

No agregar chat 1:1, sitio web, endpoint de búsqueda público, audio, imágenes,
OCR ni importación histórica sin una nueva decisión explícita.

## Arquitectura

```mermaid
flowchart TD
    http["src/http.rs<br/>recepción rápida y firmada"] --> repo["src/repository.rs"]
    repo --> db["PostgreSQL + pgvector"]
    worker["src/worker.rs"] --> repo
    worker --> wa["src/whatsapp.rs<br/>Graph API"]
    worker --> ai["src/openai.rs<br/>Responses + embeddings"]
    worker --> docs["src/document.rs<br/>extracción"]
    worker --> repo
```

`src/main.rs` sólo arma configuración, pool, migraciones, worker, router y
shutdown. La conducta testeable debe permanecer en la biblioteca.

## Invariantes

1. Verificar HMAC sobre el cuerpo original antes de parsear o acceder a la base.
2. Persistir el evento original antes de responder a Meta.
3. Mantener webhook, jobs, mensajes y respuestas salientes idempotentes.
4. No realizar red, extracción ni generación dentro del handler HTTP.
5. No procesar mensajes 1:1 ni grupos distintos al configurado.
6. No enviar respuestas para participantes fuera de la allowlist.
7. Construir RAG únicamente con fragmentos del mismo grupo y excluir la pregunta.
8. Tratar documentos y contexto RAG como entrada no confiable.
9. No registrar tokens, números completos, contenido de documentos ni secretos.
10. Agregar migraciones; nunca modificar una migración ya aplicada.

## Seguridad y secretos

`auth.json`, `.env` y cualquier archivo de credenciales son locales y están
ignorados. No mostrar su contenido en logs, respuestas, tests o commits.

Los secretos productivos viven en `/etc/agora/agora.env` en `oracle` y en el
environment GitHub `oracle`. Los secrets de aplicación nunca deben viajar como
argumentos de Docker ni guardarse en artefactos.

Al automatizar Meta con Playwright:

- crear un contexto separado usando el `storageState` local;
- no copiar cookies o tokens al repositorio;
- devolver sólo resultados sanitizados;
- no publicar la app ni enviar mensajes reales salvo que esa acción sea parte
  explícita del objetivo;
- recordar que mostrar el App Secret puede exigir reingresar la contraseña.

En `oracle`, usar `sudo -u postgres -H` para PostgreSQL y
`sudo -u deploy -H` para procesos o Docker del usuario `deploy`. No inspeccionar
entornos completos de PM2 porque pueden contener secretos de otros proyectos.

## Persistencia

Las migraciones se ejecutan con `sqlx::migrate!()` al iniciar. Todo cambio debe
preservar:

- `webhook_events.payload` como fuente original;
- claves externas únicas;
- reintentos con error sanitizado y estado dead-letter;
- borrado en cascada explícito;
- embeddings de exactamente 1536 dimensiones mientras el esquema actual siga
  vigente;
- `attachments.original_data` conserva el original y
  `attachments.content_sha256` su hash.

El worker puede ejecutar una operación más de una vez. Cada paso debe producir
el mismo resultado o detectar que ya fue completado.

## Proveedores

Usar Graph API `v25.0`, salvo migración explícita. El payload de envío grupal
debe conservar `recipient_type: "group"`.

OpenAI usa:

- `text-embedding-3-small`, 1536 dimensiones;
- `gpt-5.6-sol`;
- reasoning effort `medium`;
- Responses API con `store: false`;
- `safety_identifier` irreversible, nunca el ID real de WhatsApp.

Las respuestas deben estar en español, usar sólo el contexto recuperado, incluir
citas `[1]`, `[2]` y declarar cuando no hay evidencia.

## Calidad obligatoria

Antes de entregar:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo test --all-targets --locked
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo llvm-cov --workspace --all-features --locked --fail-under-lines 81
```

Los tests obligatorios no llaman a Meta ni OpenAI. Usar servidores HTTP locales
y PostgreSQL aislado. Una prueba que se omite
por falta de `TEST_DATABASE_URL` no demuestra integración; CI debe definirla.

Si cambian Docker, Nginx o el deploy, validar también:

```bash
AGORA_ENV_FILE=.env.example AGORA_IMAGE=agora:test \
  docker compose -f compose.production.yml config
```

## Criterio de terminado

Una tarea sólo está terminada cuando el código real, las migraciones, las
pruebas, la documentación y la evidencia externa coinciden. No marcar activos
de Meta, secretos, DNS, certificados, GitHub o producción como listos a partir
de intención o configuración local; verificar el estado real correspondiente.
