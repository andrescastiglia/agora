# Plan de convivencia entre Telegram y WhatsApp

## Objetivo

Agregar Telegram sin migrar ni eliminar WhatsApp. Agora debe soportar ambos
proveedores oficiales, con uno solo activo por instancia y Telegram como valor
predeterminado.

El proveedor se seleccionará mediante una única variable:

```env
CHAT_PROVIDER=telegram
# o
CHAT_PROVIDER=whatsapp
```

Cambiar de proveedor requerirá reiniciar o volver a desplegar el contenedor,
pero no modificar código, registrar nuevamente los webhooks ni cambiar otras
variables.

## Decisiones de diseño

- Telegram será el proveedor predeterminado.
- Sólo un proveedor podrá estar activo por instancia.
- Los webhooks de Telegram y WhatsApp permanecerán registrados.
- Los eventos del proveedor inactivo se autenticarán y responderán con `200`,
  pero no se persistirán ni procesarán.
- Las credenciales, el grupo y la allowlist serán específicos de cada
  proveedor y podrán quedar precargados simultáneamente.
- Ambos proveedores representarán el mismo espacio lógico cerrado de Agora,
  identificado por `KNOWLEDGE_SPACE_ID`.
- El conocimiento RAG se compartirá entre proveedores para conservar la
  continuidad al cambiar de plataforma.
- Cada evento, mensaje, archivo, job y respuesta guardará explícitamente su
  proveedor de origen.
- Los eventos y jobs pendientes del proveedor inactivo quedarán congelados. No
  podrán ejecutarse ni enviarse accidentalmente mediante el proveedor activo.
- WhatsApp conservará los estados `sent`, `delivered` y `read`. Telegram sólo
  podrá marcar `sent` cuando su API acepte el mensaje.

## 1. Actualizar las decisiones del producto

Actualizar `decisiones.md`, `Agents.md`, `README.md`, `TODO.md` y los avisos
legales para establecer que:

- Telegram y WhatsApp son los dos proveedores oficiales admitidos.
- Telegram es el proveedor predeterminado.
- Sólo uno se encuentra activo en cada momento.
- Ambos representan el mismo espacio lógico de conocimiento.
- No se permite mezclar grupos o participantes ajenos a ese espacio.
- Signal, WhatsApp Web y demás automatizaciones no oficiales continúan fuera de
  alcance.
- Telegram debe figurar como posible procesador de mensajería en los textos de
  privacidad, términos y consentimiento.

## 2. Neutralizar el modelo interno

Crear una interfaz común para los proveedores:

```text
src/chat/
├── mod.rs
├── telegram.rs
└── whatsapp.rs
```

Definir tipos independientes de Telegram y WhatsApp, por ejemplo:

```rust
enum ChatProvider {
    Telegram,
    WhatsApp,
}

struct IncomingMessage {
    provider: ChatProvider,
    external_message_id: String,
    conversation_id: String,
    space_id: String,
    sender_id: String,
    sender_name: Option<String>,
    text: Option<String>,
    document: Option<IncomingDocument>,
    timestamp: DateTime<Utc>,
}
```

La interfaz o despacho común deberá cubrir:

- parseo de eventos;
- descarga de documentos;
- envío de texto;
- detección de preguntas dirigidas al bot;
- procesamiento de estados cuando el proveedor los ofrezca;
- límites específicos de mensajes y archivos.

El módulo actual `src/whatsapp.rs` se moverá detrás de esta interfaz sin cambiar
su conducta externa.

## 3. Migrar la base de datos

Agregar una migración nueva. Nunca modificar las migraciones ya aplicadas.

### `webhook_events`

- conservar `provider`;
- agregar `provider_event_id`;
- deduplicar mediante `(provider, provider_event_id)`;
- usar el hash actual como ID de evento para WhatsApp;
- usar `update_id` para Telegram.

### `messages`

- agregar `provider` con backfill `whatsapp`;
- reemplazar conceptualmente `whatsapp_message_id` por
  `external_message_id`;
- agregar `conversation_id` y `space_id`;
- garantizar unicidad mediante
  `(provider, conversation_id, external_message_id)`.

### `attachments`

- agregar `provider`;
- garantizar unicidad mediante `(provider, provider_media_id)`.

### `jobs`

- agregar `provider`;
- deduplicar mediante `(provider, job_type, dedupe_key)`;
- reclamar únicamente jobs del proveedor activo.

### `outgoing_messages`

- agregar `provider`;
- hacer único `(provider, provider_message_id)`;
- conservar la relación idempotente con el mensaje de origen.

Los datos actuales se migrarán como `provider='whatsapp'`. La búsqueda RAG se
aislará mediante `space_id`, no mediante el ID técnico del grupo, para que los
documentos incorporados desde Telegram sigan disponibles al activar WhatsApp.

La migración debe ser compatible con un despliegue sobre la base productiva
existente y preservar las claves, documentos originales, jobs y respuestas.

## 4. Configuración

La configuración productiva contendrá ambos bloques:

```env
CHAT_PROVIDER=telegram
KNOWLEDGE_SPACE_ID=agora

TELEGRAM_BOT_TOKEN=
TELEGRAM_WEBHOOK_SECRET=
TELEGRAM_GROUP_ID=
TELEGRAM_ALLOWED_USER_IDS=
TELEGRAM_BOT_USERNAME=agora_telegram_bot

WHATSAPP_VERIFY_TOKEN=
WHATSAPP_APP_SECRET=
WHATSAPP_ACCESS_TOKEN=
WHATSAPP_PHONE_NUMBER_ID=
WHATSAPP_WABA_ID=
WHATSAPP_GROUP_ID=
WHATSAPP_ALLOWED_USER_IDS=
META_GRAPH_API_VERSION=v25.0
```

Reglas de configuración:

- `CHAT_PROVIDER` acepta sólo `telegram` o `whatsapp` y usa `telegram` por
  defecto.
- Si falta configuración obligatoria del proveedor seleccionado, el proceso no
  inicia.
- Las credenciales del proveedor inactivo pueden faltar en desarrollo.
- En producción se cargarán ambos bloques para que baste con cambiar
  `CHAT_PROVIDER`.
- `ALLOWED_WHATSAPP_IDS` se conservará temporalmente como alias obsoleto de
  `WHATSAPP_ALLOWED_USER_IDS` para no romper el despliegue actual.
- Los secretos nunca se registrarán ni aparecerán en errores.
- `/ready` comprobará PostgreSQL y que la configuración del proveedor activo
  esté completa, sin realizar una llamada externa en cada healthcheck.

## 5. Implementar Telegram

Agregar:

- `POST /webhooks/telegram`;
- verificación constante de
  `X-Telegram-Bot-Api-Secret-Token` antes de parsear o acceder a PostgreSQL;
- deduplicación mediante `update_id`;
- restricción estricta a `TELEGRAM_GROUP_ID`;
- restricción estricta a `TELEGRAM_ALLOWED_USER_IDS`;
- soporte de texto, captions y documentos;
- invocación mediante `@agora_telegram_bot` y `/agora`;
- descarga de archivos mediante `getFile`;
- respuesta mediante `sendMessage`, como reply al mensaje original cuando sea
  posible.

La API alojada por Telegram permite descargar archivos de hasta 20 MB. La
primera versión aplicará ese límite sólo a Telegram y mantendrá el máximo actual
de 25 MiB para WhatsApp. Si fuera indispensable admitir 25 MiB también en
Telegram, se evaluará desplegar un Bot API Server local, que permite descargas
sin ese límite.

Para que Agora incorpore mensajes y documentos que no mencionen al bot, se
desactivará Privacy Mode mediante BotFather o se otorgará al bot el rol
necesario en el grupo. El bot continuará respondiendo únicamente ante una
invocación explícita.

Los URLs que contienen el token de Telegram no deben aparecer en logs, errores
ni trazas.

## 6. Adaptar HTTP y el worker

El router expondrá ambos webhooks. Cada handler deberá:

1. autenticar la solicitud antes de parsearla;
2. comprobar si su proveedor está activo;
3. responder `200` sin persistir si está inactivo;
4. persistir el evento original y su proveedor si está activo;
5. responder rápidamente sin realizar descarga, extracción ni generación.

El worker deberá:

1. reclamar sólo eventos del proveedor activo;
2. convertir el payload mediante el parser correspondiente;
3. persistir un mensaje neutral con `provider`, `conversation_id` y `space_id`;
4. encolar jobs con su proveedor;
5. descargar archivos usando el proveedor guardado en el job;
6. buscar contexto por `space_id`;
7. enviar respuestas usando el proveedor guardado en la respuesta;
8. dejar congelado cualquier trabajo de un proveedor inactivo.

La selección del cliente nunca debe inferirse únicamente desde la configuración
actual cuando el trabajo ya está persistido. El proveedor almacenado en el
evento o job será autoritativo.

## 7. Pruebas obligatorias

Agregar pruebas para:

- valor predeterminado `telegram`;
- selección y validación de ambos proveedores;
- redacción de todos los secretos;
- secreto inválido de Telegram rechazado antes de PostgreSQL;
- deduplicación de `update_id`;
- IDs iguales permitidos en proveedores diferentes;
- aislamiento por grupo y allowlist;
- parseo de texto, comandos, menciones, captions y documentos;
- rechazo de chats privados y grupos distintos;
- límite de archivos de Telegram;
- descarga y envío contra un servidor HTTP local;
- jobs del proveedor inactivo no reclamados;
- trabajos ya reclamados que conservan su proveedor;
- conocimiento compartido por `space_id`;
- cambio Telegram a WhatsApp modificando únicamente `CHAT_PROVIDER`;
- migración de todos los registros existentes como WhatsApp;
- idempotencia de eventos, mensajes, jobs y respuestas;
- ausencia de llamadas reales a Telegram, Meta u OpenAI.

Ejecutar las validaciones obligatorias:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo test --all-targets --locked
TEST_DATABASE_URL=postgres://agora:agora@localhost:5432/agora \
  cargo llvm-cov --workspace --all-features --locked --fail-under-lines 81
```

Si cambia el despliegue, validar también:

```bash
AGORA_ENV_FILE=.env.example AGORA_IMAGE=agora:test \
  docker compose -f compose.production.yml config
```

## 8. Aprovisionamiento y despliegue

1. Crear el bot con BotFather.
2. Desactivar Privacy Mode.
3. Crear el grupo privado y agregar al bot y a los seis participantes.
4. Obtener el ID del grupo y los IDs de usuario sin registrarlos ni
   versionarlos.
5. Registrar una sola vez el webhook HTTPS de Telegram con su secreto.
6. Mantener registrado el webhook existente de WhatsApp.
7. Cargar ambos bloques de secretos en `/etc/agora/agora.env` con los permisos
   actuales.
8. Desplegar con `CHAT_PROVIDER=telegram`.
9. Probar texto, documento, RAG, citas, duplicados y reinicio.
10. Confirmar que los eventos de WhatsApp son aceptados pero ignorados mientras
    Telegram está activo.
11. Cambiar temporalmente a `CHAT_PROVIDER=whatsapp`, reiniciar y comprobar que:
    - Telegram deja de persistirse y procesarse;
    - los jobs de Telegram quedan congelados;
    - `/ready` valida la configuración de WhatsApp;
    - no se envían mensajes de Telegram mediante WhatsApp;
    - al volver a Telegram, sus jobs continúan correctamente.

## 9. Entrega por etapas

### Etapa A: modelo neutral

- Decisiones y configuración actualizadas.
- Migración aplicada y datos existentes preservados.
- WhatsApp funciona detrás de la interfaz común sin regresiones.

### Etapa B: Telegram

- Webhook, parser, descarga y envío implementados.
- Aislamiento de grupo y participantes probado.
- Suite completa y cobertura obligatoria en verde.

### Etapa C: producción

- Bot y grupo creados.
- Secretos cargados fuera de Git.
- Flujo real con texto y documentos validado.
- Cambio controlado entre proveedores validado.
- Documentación y evidencia externa actualizadas.

## Criterio de terminado

La funcionalidad estará completa cuando, con ambos proveedores previamente
configurados, éste sea el único cambio necesario:

```diff
-CHAT_PROVIDER=telegram
+CHAT_PROVIDER=whatsapp
```

Después de reiniciar el contenedor, Agora deberá operar sobre WhatsApp sin
mezclar eventos, grupos, participantes, jobs o respuestas, y deberá conservar el
mismo espacio de conocimiento. El cambio inverso deberá ofrecer las mismas
garantías.
