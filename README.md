# Agora

Agora es un bot privado para un grupo cerrado de WhatsApp. Recibe mensajes y
documentos mediante la API oficial de Meta, construye una base de conocimiento
en español y responde dentro del mismo grupo cuando una persona escribe
`@agora` seguida de una pregunta.

No tiene sitio, login ni interfaz web. El dominio `agora.maese.com.ar` existe
para el webhook HTTPS de Meta, los health checks y los avisos legales públicos.

## Funcionamiento

```mermaid
flowchart LR
    group["Grupo de WhatsApp"] --> meta["WhatsApp Cloud / Groups API"]
    meta -->|webhook firmado| api["Agora<br/>Rust + Axum"]
    api --> events["PostgreSQL<br/>eventos y cola"]
    events --> worker["Worker idempotente"]
    worker --> objects["OCI Object Storage<br/>archivos originales"]
    worker --> openai["OpenAI<br/>embeddings + respuesta"]
    worker --> knowledge["PostgreSQL + pgvector<br/>texto y fragmentos"]
    knowledge --> worker
    worker -->|respuesta con citas| meta
```

El handler valida `X-Hub-Signature-256`, conserva el JSON original y responde
rápidamente. El worker procesa el evento después, con deduplicación, reintentos
exponenciales y estado dead-letter.

Contenido admitido en la versión 1:

- texto;
- documentos `.doc`, `.docx`, `.pdf`, `.xls` y `.xlsx`;
- español;
- mensajes del único grupo configurado y de los seis participantes autorizados.

No se admiten conversaciones individuales, audio, imágenes, OCR, importación
histórica ni búsquedas desde una API pública.

## Estado

Implementado:

- verificación y firma del webhook de Meta;
- persistencia idempotente del evento original;
- parser tipado de mensajes grupales, documentos y estados de entrega;
- cola PostgreSQL con `FOR UPDATE SKIP LOCKED`, reintentos y dead letters;
- descarga limitada de medios y extracción aislada sin ejecutar un shell;
- almacenamiento de documentos por hash en OCI Object Storage;
- chunking, embeddings, búsqueda híbrida por grupo y respuestas RAG con citas;
- envío grupal oficial y deduplicación de respuestas salientes;
- health, readiness y avisos de privacidad, términos y eliminación;
- pruebas sin llamadas reales a Meta/OpenAI y cobertura de líneas superior a
  81%;
- workflows de CI, publicación multi-arquitectura en GHCR y despliegue
  automático con rollback en `oracle`.

La puesta en producción depende además de activos externos. El estado verificable
y los bloqueos se mantienen en [`TODO.md`](TODO.md).

## Desarrollo local

Requisitos:

- Rust 1.97, instalado automáticamente por `rust-toolchain.toml`;
- Docker y Docker Compose;
- `pdftotext`, LibreOffice y `antiword` para probar extracción fuera del
  contenedor.

Iniciar PostgreSQL:

```bash
docker compose up -d postgres
```

Crear la configuración:

```bash
cp .env.example .env
```

Como mínimo, reemplazá los valores de `DATABASE_URL`,
`WHATSAPP_VERIFY_TOKEN` y `WHATSAPP_APP_SECRET`. Luego:

```bash
cargo run
```

El servicio escucha en `http://localhost:8080` de forma predeterminada.

## Endpoints públicos

| Método | Ruta | Finalidad |
| --- | --- | --- |
| `GET` | `/health` | Indica que el proceso está activo |
| `GET` | `/ready` | Comprueba la conexión a PostgreSQL |
| `GET` | `/webhooks/whatsapp` | Desafío de verificación de Meta |
| `POST` | `/webhooks/whatsapp` | Recepción firmada de eventos |
| `GET` | `/privacy` | Política de privacidad |
| `GET` | `/terms` | Términos de uso |
| `GET` | `/data-deletion` | Instrucciones de exportación y eliminación |

No existe un endpoint de búsqueda: las consultas se hacen exclusivamente desde
WhatsApp.

## Configuración

Secretos obligatorios para iniciar:

| Variable | Uso |
| --- | --- |
| `DATABASE_URL` | PostgreSQL con pgvector |
| `WHATSAPP_VERIFY_TOKEN` | Desafío inicial del webhook |
| `WHATSAPP_APP_SECRET` | Verificación HMAC de cada evento |

Integración de WhatsApp:

| Variable | Uso |
| --- | --- |
| `WHATSAPP_ACCESS_TOKEN` | Token del system user |
| `WHATSAPP_PHONE_NUMBER_ID` | Número emisor de Cloud API |
| `WHATSAPP_WABA_ID` | Cuenta de WhatsApp Business |
| `WHATSAPP_GROUP_ID` | Único grupo autorizado |
| `ALLOWED_WHATSAPP_IDS` | IDs separados por coma de participantes |
| `META_GRAPH_API_VERSION` | Versión fijada, por defecto `v25.0` |
| `BOT_MENTION` | Prefijo, por defecto `@agora` |

OpenAI:

| Variable | Predeterminado |
| --- | --- |
| `OPENAI_API_KEY` | Sin valor |
| `OPENAI_RESPONSE_MODEL` | `gpt-5.6-sol` |
| `OPENAI_EMBEDDING_MODEL` | `text-embedding-3-small` |
| `OPENAI_EMBEDDING_DIMENSIONS` | `1536` |

OCI Object Storage:

| Variable | Uso |
| --- | --- |
| `OCI_OBJECT_STORAGE_ENDPOINT` | Endpoint S3 compatible del namespace |
| `OCI_OBJECT_STORAGE_REGION` | Región OCI |
| `OCI_OBJECT_STORAGE_BUCKET` | Bucket privado de documentos |
| `OCI_OBJECT_STORAGE_ACCESS_KEY_ID` | Customer Secret Key ID |
| `OCI_OBJECT_STORAGE_SECRET_ACCESS_KEY` | Customer Secret Key |

Los límites y valores restantes están documentados en
[`.env.example`](.env.example). Ningún secreto debe entrar en Git.

## Pruebas y calidad

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo test --all-targets --locked
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo llvm-cov --workspace --all-features --locked --fail-under-lines 81
```

El test de integración se omite localmente si `TEST_DATABASE_URL` no está
definida. CI siempre inicia PostgreSQL con pgvector y lo ejecuta.

## Producción

Cada push a `main`, que debe provenir de un PR con CI verde:

1. construye imágenes `linux/amd64` y `linux/arm64`;
2. publica un tag por SHA y un digest inmutable en GHCR;
3. genera una attestación de procedencia;
4. conecta por SSH al environment `oracle`;
5. aplica `compose.production.yml`;
6. espera `/ready` y vuelve al digest anterior si falla.

Nginx sólo publica `80/443`; la API escucha en `127.0.0.1:8088` y PostgreSQL en
`127.0.0.1:5432`. Los scripts operativos están en [`scripts`](scripts).

## Restricción de Meta

WhatsApp Groups API requiere una Official Business Account y crea grupos
programáticamente. No permite convertir silenciosamente una Community existente
ni usar automatizaciones no oficiales de WhatsApp Web. Por eso el lanzamiento
queda condicionado a que Meta otorgue elegibilidad y a una prueba real con el
número productivo.
