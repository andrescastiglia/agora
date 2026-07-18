# Agora

**Company brain privado para comunidades y grupos cerrados de WhatsApp**, implementado en Rust.

Agora recibe mensajes mediante WhatsApp Cloud API, conserva el evento original y prepara una tubería para normalizar texto, transcribir audio, extraer documentos y generar embeddings consultables con PostgreSQL + pgvector.

## Arquitectura

```text
Comunidad / grupos de WhatsApp
               │
               ▼
      WhatsApp Cloud API
               │
               ▼
            Webhook
               │
               ▼
     Servicio de ingestión
         Rust + Axum
               │
     ┌─────────┼──────────┐
     ▼         ▼          ▼
  Textos     Audios    Documentos
     │         │          │
     │    Transcripción   Extracción
     │      OpenAI        de texto
     └─────────┼──────────┘
               ▼
       Normalización
               │
               ▼
    PostgreSQL + pgvector
               │
               ▼
       API de consultas
               │
       ┌───────┴────────┐
       ▼                ▼
  Chat web         Bot WhatsApp
```

## Estado actual

- API HTTP con Axum.
- `GET /health`.
- Verificación de webhook de Meta mediante `GET /webhooks/whatsapp`.
- Recepción y persistencia de eventos mediante `POST /webhooks/whatsapp`.
- Migraciones SQL para eventos, mensajes, chunks y embeddings de 1536 dimensiones.
- Índice HNSW con distancia coseno.
- Stack local con Docker Compose.
- Endpoint reservado `POST /api/search` para la siguiente etapa.

## Inicio local

```bash
cp .env.example .env
# Editar WHATSAPP_VERIFY_TOKEN y las demás credenciales.
docker compose up --build
```

Verificación:

```bash
curl http://localhost:8080/health
```

Respuesta esperada:

```json
{"status":"ok"}
```

## Desarrollo sin Docker

Requisitos: Rust estable, PostgreSQL y la extensión pgvector.

```bash
cp .env.example .env
cargo run
```

Las migraciones se ejecutan automáticamente al iniciar el servicio.

## Próximos hitos

1. Validación de firma `X-Hub-Signature-256`.
2. Parser tipado del payload de WhatsApp.
3. Cola persistente y workers de ingestión.
4. Descarga segura de medios desde Meta.
5. Transcripción de audio y extracción de documentos.
6. Chunking, embeddings y búsqueda semántica.
7. API RAG, chat web y respuestas por WhatsApp.

## Seguridad

El repositorio no debe contener tokens ni secretos. Use variables de entorno y mantenga `.env` fuera del control de versiones.
