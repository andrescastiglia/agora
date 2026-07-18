# TODO — Puesta en producción de Agora

Última auditoría: 18 de julio de 2026.

Esta lista refleja el alcance acordado en `decisiones.md`. Las referencias a una
interfaz web, chat 1:1, audio, OCR o importación histórica fueron eliminadas
porque no pertenecen a la versión 1.

## 1. Backend

- [x] Separar biblioteca, binario, configuración, HTTP, repositorio y worker.
- [x] Fijar Rust 1.97 y versionar `Cargo.lock`.
- [x] Validar configuración sin revelar secretos.
- [x] Ejecutar migraciones compatibles al iniciar.
- [x] Recibir el cuerpo original y limitarlo a 1 MiB.
- [x] Validar `X-Hub-Signature-256` con comparación constante.
- [x] Persistir y deduplicar webhooks antes de responder.
- [x] Parsear mensajes grupales, documentos, contactos y estados.
- [x] Ignorar 1:1 y grupos distintos de `WHATSAPP_GROUP_ID`.
- [x] Restringir respuestas a `ALLOWED_WHATSAPP_IDS`.
- [x] Implementar jobs PostgreSQL con `SKIP LOCKED`, reintentos y dead-letter.
- [x] Descargar medios con timeout y límite de 25 MiB.
- [x] Admitir DOC, DOCX, PDF, XLS y XLSX sin ejecutar un shell.
- [x] Eliminar archivos temporales incluso ante errores.
- [x] Guardar originales y su hash en PostgreSQL (`BYTEA`), con límite de 25 MiB.
- [x] Normalizar, fragmentar y generar embeddings de 1536 dimensiones.
- [x] Implementar búsqueda híbrida textual/vectorial aislada por grupo.
- [x] Generar en español con citas y defensa contra instrucciones en fuentes.
- [x] Enviar respuestas grupales oficiales e idempotentes.
- [x] Aplicar estados salientes sin retroceder ante eventos fuera de orden.
- [x] Exponer `/health`, `/ready` y los tres avisos legales.

## 2. Pruebas y calidad

- [x] Probar configuración, firma, challenge, límites y errores HTTP.
- [x] Probar parsers de grupos, documentos y estados.
- [x] Probar clientes Meta y OpenAI con servidores locales.
- [x] Probar la persistencia binaria contra PostgreSQL.
- [x] Probar migraciones, idempotencia, jobs, búsqueda y estados contra pgvector.
- [x] Ejecutar `cargo fmt --check`.
- [x] Ejecutar Clippy con `-D warnings`.
- [x] Alcanzar cobertura de líneas mayor a 81%: evidencia local `87,11%`.
- [x] Mantener la suite obligatoria sin llamadas externas.
- [x] Confirmar CI verde en GitHub y publicar el artefacto LCOV (run
  `29630018119`, artefacto `coverage-lcov`).

## 3. GitHub

- [x] Crear CI para PR con formato, Clippy, tests, cobertura, auditoría e imagen.
- [x] Fijar las acciones por SHA y permisos mínimos.
- [x] Configurar Dependabot para Cargo, Actions y Docker.
- [x] Crear build multi-arquitectura y publicación en GHCR por SHA/digest.
- [x] Crear attestación de procedencia.
- [x] Crear deploy automático serializado con rollback.
- [x] Crear el environment `oracle` sin aprobación manual.
- [x] Cargar secrets SSH y `ORACLE_DEPLOY_PATH=/opt/agora`.
- [x] Proteger `main`: PR obligatorio, cero aprobaciones, checks obligatorios,
  sin force push ni eliminación.
- [x] Abrir PR, obtener CI verde y mergear a `main` (PR #1 y correcciones
  operativas #7/#8).
- [x] Confirmar que la imagen GHCR queda pública y puede inspeccionarse sin
  autenticación con plataformas `linux/amd64` y `linux/arm64`.

## 4. Oracle

- [x] Verificar Ubuntu ARM64, Docker/Compose, Nginx, Certbot y espacio.
- [x] Verificar PostgreSQL 17, pgvector y escucha exclusiva en localhost.
- [x] Diseñar runtime aislado en `127.0.0.1:8088`.
- [x] Crear Compose con usuario no root, filesystem read-only, límites, logs y
  healthcheck.
- [x] Crear deploy idempotente por digest con rollback.
- [x] Crear aprovisionamiento Nginx que no modifica otros virtual hosts.
- [x] Crear base y usuario PostgreSQL `agora` con contraseña aleatoria.
- [x] Crear `/etc/agora/agora.env` con permisos restringidos.
- [x] Instalar virtual host y certificado TLS de `agora.maese.com.ar`.
- [x] Desplegar una imagen GHCR inmutable y verificar `/health` y `/ready`
  (run `29634781853`, digest `sha256:55694067985de1948f97198824a67157d4b6b847f762b2590f073c601ea0a850`).
- [x] Implementar backup local cifrado de PostgreSQL y probar restauración.
- [x] Confirmar que sólo Nginx `80/443` publica Agora; API y PostgreSQL quedan en
  loopback.

## 5. Meta

- [x] Confirmar app `Agora` y Business Portfolio mediante `auth.json`.
- [x] Confirmar que el caso de uso WhatsApp está agregado.
- [x] Configurar categoría `Messaging`, ícono y URLs públicas legales de la app.
- [x] Verificar requisitos oficiales de Groups API al 17/07/2026.
- [x] Confirmar límite de participantes compatible.
- [x] Confirmar que Groups API no vincula una Community existente.
- [ ] Obtener elegibilidad Official Business Account.
- [x] Completar verificación del negocio y 2FA exigida (Business Portfolio y
  Tech Provider verificados; 2FA requerida para todos).
- [x] Recuperar App Secret y cargarlo sin exponerlo; se validó con un webhook
  firmado por Meta en producción.
- [x] Agregar y verificar el número productivo en Cloud API (`CONNECTED`,
  `code_verification_status=VERIFIED`; la revisión del nuevo nombre sigue en
  curso y Groups API devuelve `131215` por falta de elegibilidad).
- [x] Crear system user y token permanente con permisos mínimos (`agora`,
  `SYSTEM_USER`, válido, `expires_at=0`, WABA y número accesibles).
- [x] Inventariar WABA ID y Phone Number ID fuera de Git, en
  `/etc/agora/agora.env`.
- [ ] Crear el grupo oficial por Groups API e invitar a los seis participantes.
- [x] Configurar callback `https://agora.maese.com.ar/webhooks/whatsapp`.
- [x] Suscribir `messages`, `group_lifecycle_update`,
  `group_participants_update`, `group_settings_update` y
  `group_status_update`.
- [x] Configurar URLs de privacidad, términos y eliminación.
- [x] Verificar challenge real y webhook firmado desde el panel de Meta; una
  segunda entrega idéntica fue deduplicada.
- [ ] Probar mensaje entrante, documento, respuesta y estados.
- [ ] Publicar la app después de elegibilidad OBA y revisión legal, pero antes
  del piloto real: Meta no entrega eventos productivos mientras está sin
  publicar.

## 6. Secretos y consentimiento

- [x] Cargar `OPENAI_API_KEY` directamente en `oracle` y validarla contra la API
  sin exponerla (`HTTP 200`).
- [x] Cargar App Secret, token permanente, WABA ID y Phone Number ID
  directamente en `oracle`.
- [ ] Cargar Group ID y `ALLOWED_WHATSAPP_IDS` directamente en `oracle` cuando
  existan el grupo elegible y los consentimientos.
- [x] Preparar un formulario versionado de consentimiento sin datos personales.
- [ ] Documentar consentimiento de los seis participantes.
- [x] Publicar política de privacidad propuesta.
- [x] Publicar términos y procedimiento de exportación/eliminación.
- [ ] Revisar y aprobar legalmente los textos propuestos, incluido que el RAG
  cerrado y limitado de Agora no infringe las condiciones de Meta para
  proveedores o asistentes de IA.

## 7. Prueba final

- [x] Firma inválida devuelve `401` en producción.
- [ ] Evento real se persiste una sola vez (el webhook de prueba firmado del
  panel de Meta ya demostró persistencia y deduplicación).
- [ ] Documento real queda en PostgreSQL, se extrae y se indexa.
- [ ] `@agora` responde dentro del grupo con citas.
- [ ] Reiniciar el contenedor no pierde ni duplica jobs.
- [x] Un despliegue inválido vuelve al digest anterior (digest inexistente
  rechazado, rollback ejecutado y `/ready` continuó saludable).
- [x] Un backup local se restaura en una base aislada.
- [x] Merge de PR a `main` publica y despliega exactamente un digest (run
  `29634781853`; el índice público, `.deployed-image` y el contenedor coinciden).
- [ ] Todos los participantes dieron consentimiento.

Agora estará completo cuando no quede ninguna casilla abierta y la evidencia
externa confirme Meta, GitHub, `oracle`, OpenAI y el flujo real.
