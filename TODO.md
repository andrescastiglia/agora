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
- [x] Guardar originales por hash en OCI Object Storage.
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
- [x] Probar OCI con un object store en memoria.
- [x] Probar migraciones, idempotencia, jobs, búsqueda y estados contra pgvector.
- [x] Ejecutar `cargo fmt --check`.
- [x] Ejecutar Clippy con `-D warnings`.
- [x] Alcanzar cobertura de líneas mayor a 81%: evidencia local `83,86%`.
- [x] Mantener la suite obligatoria sin llamadas externas.
- [ ] Confirmar CI verde en GitHub y publicar el artefacto LCOV.

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
- [ ] Abrir PR, obtener CI verde y mergear a `main`.
- [ ] Confirmar que la imagen GHCR queda pública.

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
- [ ] Desplegar una imagen GHCR inmutable y verificar `/health` y `/ready`
  (el bootstrap ARM64 local ya está saludable).
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
- [ ] Completar verificación del negocio y 2FA exigida.
- [ ] Reingresar contraseña y recuperar App Secret sin exponerlo.
- [ ] Agregar y verificar el número productivo en Cloud API.
- [ ] Crear system user y token permanente con permisos mínimos.
- [ ] Inventariar WABA ID y Phone Number ID fuera de Git.
- [ ] Crear el grupo oficial por Groups API e invitar a los seis participantes.
- [ ] Configurar callback `https://agora.maese.com.ar/webhooks/whatsapp`.
- [ ] Suscribir `messages`, `group_lifecycle_update`,
  `group_participants_update`, `group_settings_update` y
  `group_status_update`.
- [x] Configurar URLs de privacidad, términos y eliminación.
- [ ] Verificar challenge real y webhook firmado.
- [ ] Probar mensaje entrante, documento, respuesta y estados.
- [ ] Publicar la app sólo después del piloto y revisión.

## 6. Secretos y consentimiento

- [ ] Cargar `OPENAI_API_KEY` directamente en `oracle`.
- [ ] Crear bucket privado OCI y Customer Secret Key.
- [ ] Cargar endpoint, región, bucket y claves OCI en `oracle`.
- [ ] Cargar App Secret, token, IDs y allowlist directamente en `oracle`.
- [ ] Documentar consentimiento de los seis participantes.
- [x] Publicar política de privacidad propuesta.
- [x] Publicar términos y procedimiento de exportación/eliminación.
- [ ] Revisar y aprobar legalmente los textos propuestos.

## 7. Prueba final

- [ ] Firma inválida devuelve `401` en producción.
- [ ] Evento real se persiste una sola vez.
- [ ] Documento real queda en OCI, se extrae y se indexa.
- [ ] `@agora` responde dentro del grupo con citas.
- [ ] Reiniciar el contenedor no pierde ni duplica jobs.
- [ ] Un despliegue inválido vuelve al digest anterior.
- [x] Un backup local se restaura en una base aislada.
- [ ] Merge de PR a `main` publica y despliega exactamente un digest.
- [ ] Todos los participantes dieron consentimiento.

Agora estará completo cuando no quede ninguna casilla abierta y la evidencia
externa confirme Meta, GitHub, `oracle`, OCI, OpenAI y el flujo real.
