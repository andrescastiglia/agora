# Agora

Agora es un bot privado para un espacio de conocimiento cerrado que puede
operar mediante Telegram o WhatsApp. Ambos proveedores oficiales pueden quedar
configurados simultáneamente, pero sólo uno está activo por instancia. Telegram
es el predeterminado y el cambio se realiza con `CHAT_PROVIDER` y un reinicio.

Recibe texto y documentos, construye una base de conocimiento en español y
responde en el mismo grupo ante `@agora_telegram_bot` o `/agora` en Telegram y `@agora`
en WhatsApp. El conocimiento se comparte entre plataformas mediante
`KNOWLEDGE_SPACE_ID`; eventos, participantes, jobs y respuestas permanecen
aislados por proveedor.

No tiene sitio, login ni interfaz web. `agora.maese.com.ar` existe para los
webhooks HTTPS, health checks y avisos legales.

## Funcionamiento

```mermaid
flowchart LR
    telegram["Grupo privado de Telegram"] --> api["Agora<br/>Rust + Axum"]
    whatsapp["Grupo oficial de WhatsApp"] --> api
    api --> events["PostgreSQL + pgvector<br/>eventos, cola, originales y conocimiento"]
    events --> worker["Worker idempotente<br/>sólo proveedor activo"]
    worker --> openai["OpenAI<br/>embeddings + respuesta"]
    worker --> telegram
    worker --> whatsapp
```

Ambos webhooks permanecen publicados. Cada solicitud se autentica antes de ser
parseada; si pertenece al proveedor inactivo responde `200` sin persistirse. El
worker reclama exclusivamente eventos y jobs del proveedor activo. Cambiar de
proveedor congela el trabajo pendiente anterior y lo continúa al volver.
El payload original se minimiza al completar o descartar definitivamente el
evento; el proveedor, su ID externo y el hash permanecen para deduplicación.

Contenido admitido:

- texto y captions;
- documentos `.doc`, `.docx`, `.pdf`, `.xls` y `.xlsx`;
- máximo 20 MiB en Telegram y 25 MiB en WhatsApp;
- español;
- un grupo y una allowlist específicos por proveedor.

No se admiten chats privados, audio, imágenes, OCR, importación histórica,
Signal, WhatsApp Web ni una API pública de búsqueda.

## Desarrollo local

Requisitos:

- Rust 1.97;
- Docker y Docker Compose;
- `pdftotext`, LibreOffice y `antiword` para extracción fuera del contenedor.

```bash
docker compose up -d postgres
cp .env.example .env
cargo run
```

Completá `DATABASE_URL`, `KNOWLEDGE_SPACE_ID` y todo el bloque del proveedor
seleccionado. Las credenciales del proveedor inactivo pueden quedar vacías en
el entorno local. En `oracle` se cargan ambos bloques para que el único cambio
sea:

```env
CHAT_PROVIDER=telegram
# o CHAT_PROVIDER=whatsapp
```

## Endpoints públicos

| Método | Ruta | Finalidad |
| --- | --- | --- |
| `GET` | `/health` | Proceso activo |
| `GET` | `/ready` | PostgreSQL y configuración del proveedor y OpenAI completas |
| `POST` | `/webhooks/telegram` | Updates autenticados de Telegram |
| `GET` | `/webhooks/whatsapp` | Challenge de Meta |
| `POST` | `/webhooks/whatsapp` | Eventos firmados de WhatsApp |
| `GET` | `/privacy` | Política de privacidad |
| `GET` | `/terms` | Términos de uso |
| `GET` | `/data-deletion` | Exportación y eliminación |

## Configuración

Común:

| Variable | Uso |
| --- | --- |
| `DATABASE_URL` | PostgreSQL 17 con pgvector |
| `CHAT_PROVIDER` | `telegram` (predeterminado) o `whatsapp` |
| `KNOWLEDGE_SPACE_ID` | Espacio RAG compartido por ambos proveedores |

Telegram:

| Variable | Uso |
| --- | --- |
| `TELEGRAM_BOT_TOKEN` | Token de BotFather |
| `TELEGRAM_WEBHOOK_SECRET` | Verificación constante del webhook |
| `TELEGRAM_GROUP_ID` | Único grupo o supergrupo admitido |
| `TELEGRAM_ALLOWED_USER_IDS` | IDs autorizados separados por coma |
| `TELEGRAM_BOT_USERNAME` | Username usado en menciones |

WhatsApp:

| Variable | Uso |
| --- | --- |
| `WHATSAPP_VERIFY_TOKEN` | Challenge de Meta |
| `WHATSAPP_APP_SECRET` | HMAC del webhook |
| `WHATSAPP_ACCESS_TOKEN` | Token del system user |
| `WHATSAPP_PHONE_NUMBER_ID` | Número emisor |
| `WHATSAPP_WABA_ID` | Cuenta de WhatsApp Business |
| `WHATSAPP_GROUP_ID` | Único grupo admitido |
| `WHATSAPP_ALLOWED_USER_IDS` | Participantes autorizados |
| `ALLOWED_WHATSAPP_IDS` | Alias obsoleto de la variable anterior |
| `META_GRAPH_API_VERSION` | `v25.0` por defecto |

OpenAI usa `gpt-5.6-sol`, `text-embedding-3-small`, 1536 dimensiones y Responses
API con almacenamiento desactivado. Todos los valores se enumeran en
[`.env.example`](.env.example); ningún secreto entra en Git o en logs.

## Pruebas y calidad

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo test --all-targets --locked
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo llvm-cov --workspace --all-features --locked --fail-under-lines 81
```

Las pruebas usan servidores HTTP locales y PostgreSQL/pgvector; no llaman a
Telegram, Meta ni OpenAI reales.

Para una base que tenga aplicada `0007` pero no `0008`, ejecutar antes del
despliegue el preflight documentado en
[`scripts/preflight-chat-provider-migration.sql`](scripts/preflight-chat-provider-migration.sql).
Normaliza estados nulos permitidos por el esquema anterior; la suite prueba la
cadena completa hasta `0011`.

## Oracle

`main` es la única rama del repositorio. Cada push ejecuta CI, pero no despliega.
El único despliegue se inicia al crear sobre el último commit de `main` un tag
con formato exacto `vX.X.X` y publicarlo en GitHub:

```bash
git tag v0.3.0
git push origin v0.3.0
```

El workflow vuelve a ejecutar formato, Clippy y pruebas, publica la imagen
ARM64/AMD64 con el tag de versión y despliega su digest en el único environment
`oracle`, con readiness y rollback. Nginx publica únicamente `80/443`; Agora y
PostgreSQL escuchan en loopback.

Cada respuesta saliente realiza un solo intento automático. Si el proveedor
acepta la solicitud pero la confirmación de red es ambigua, queda en
`delivery_unknown` para revisión manual y no se reenvía automáticamente. Los
estados confirmados avanzan de forma monótona.

El código soporta ambos proveedores. La habilitación real de Telegram requiere
crear el bot y grupo, desactivar Privacy Mode o hacerlo administrador, registrar
el webhook con secreto y cargar las credenciales fuera de Git. WhatsApp continúa
condicionado por la elegibilidad de Groups API documentada en [TODO.md](TODO.md).

Una vez cargados `TELEGRAM_BOT_TOKEN` y `TELEGRAM_WEBHOOK_SECRET` en el archivo
protegido del servidor, el webhook se registra y verifica sin exponer esos
secretos en la línea de comandos:

```bash
sudo -u deploy /opt/agora/configure-telegram-webhook.sh /etc/agora/agora.env
sudo -u deploy /opt/agora/configure-telegram-webhook.sh --check /etc/agora/agora.env
```

El primer comando configura la URL exacta y los tipos de update admitidos; el
segundo sólo comprueba que Telegram sigue apuntando al endpoint esperado.

## Derechos de las personas

El procedimiento operativo de acceso, corrección, exportación y eliminación,
incluido el tratamiento de backups, está en [DATA_RIGHTS.md](DATA_RIGHTS.md).
Los scripts se despliegan en `/opt/agora` y requieren identidad verificada y
ejecución explícita como `root`.
