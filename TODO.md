# TODO — Puesta en producción de Agora

Última auditoría: 31 de julio de 2026.

Esta lista refleja el alcance acordado en `decisiones.md`: Telegram y WhatsApp
conviven como proveedores oficiales de un mismo espacio, con sólo uno activo.
Interfaz web, chat 1:1, Signal, WhatsApp Web, audio, OCR e importación histórica
no pertenecen a la versión 1.

## 1. Backend

- [x] Separar biblioteca, binario, configuración, HTTP, repositorio y worker.
- [x] Neutralizar mensajes, adjuntos, eventos, jobs y salidas por proveedor.
- [x] Seleccionar un único proveedor con Telegram como valor predeterminado.
- [x] Compartir conocimiento por `KNOWLEDGE_SPACE_ID` sin mezclar grupos ni jobs.
- [x] Fijar Rust 1.97 y versionar `Cargo.lock`.
- [x] Validar configuración sin revelar secretos.
- [x] Ejecutar migraciones compatibles al iniciar.
- [x] Recibir el cuerpo original y limitarlo a 1 MiB.
- [x] Validar `X-Hub-Signature-256` con comparación constante.
- [x] Autenticar ambos webhooks, ignorar el inactivo y deduplicar por proveedor.
- [x] Parsear mensajes grupales, documentos, contactos y estados.
- [x] Ignorar 1:1 y grupos distintos del configurado para cada proveedor.
- [x] Restringir respuestas a la allowlist específica del proveedor.
- [x] Implementar jobs PostgreSQL con `SKIP LOCKED`, reintentos y dead-letter.
- [x] Descargar medios con timeout y límite de 25 MiB.
- [x] Admitir DOC, DOCX, PDF, XLS y XLSX sin ejecutar un shell.
- [x] Eliminar archivos temporales incluso ante errores.
- [x] Guardar originales y su hash en PostgreSQL (`BYTEA`), con límite de 25 MiB.
- [x] Normalizar, fragmentar y generar embeddings de 1536 dimensiones.
- [x] Implementar búsqueda híbrida textual/vectorial aislada por espacio lógico.
- [x] Generar en español con citas y defensa contra instrucciones en fuentes.
- [x] Enviar respuestas grupales oficiales e idempotentes.
- [x] Aplicar estados salientes sin retroceder ante eventos fuera de orden.
- [x] Exponer `/health`, `/ready` y los tres avisos legales.

## 2. Pruebas y calidad

- [x] Probar configuración, firma, challenge, límites y errores HTTP.
- [x] Probar parsers de grupos, documentos y estados.
- [x] Probar clientes Meta y OpenAI con servidores locales.
- [x] Probar parser, descarga y envío de Telegram con servidores locales.
- [x] Probar congelamiento de eventos/jobs inactivos e IDs iguales entre proveedores.
- [x] Probar migración integral de registros históricos como WhatsApp.
- [x] Probar la persistencia binaria contra PostgreSQL.
- [x] Probar migraciones, idempotencia, jobs, búsqueda y estados contra pgvector.
- [x] Ejecutar `cargo fmt --check`.
- [x] Ejecutar Clippy con `-D warnings`.
- [x] Alcanzar cobertura de líneas mayor a 81%: evidencia local `88,10%` al
  18/07/2026.
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
- [x] Abrir PR, obtener CI verde y mergear a `main` (PR #1, correcciones
  operativas #7/#8 y cierre de preparación #11).
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
  (último run `29668526663`, digest
  `sha256:a5c411136c9a39652a7befd5259973822763aa9f26cd8bdf35f9fb7111eb6140`;
  contenedor saludable y sin reinicios al 29/07/2026).
- [x] Implementar backup local cifrado de PostgreSQL y probar restauración.
- [x] Confirmar que sólo Nginx `80/443` publica Agora; API y PostgreSQL quedan en
  loopback.

## 5. Telegram

- [x] Implementar `POST /webhooks/telegram` con secreto constante antes del parseo.
- [x] Deduplicar `update_id` y filtrar grupo, chat privado y allowlist.
- [x] Admitir texto, captions, documentos, `@agora_telegram_bot` y `/agora`.
- [x] Implementar `getFile`, descarga limitada a 20 MiB y `sendMessage` como reply.
- [x] Automatizar el registro y la verificación segura del webhook sin exponer el token.
- [x] Crear el bot productivo `@agora_telegram_bot`, verificar su identidad mediante
  `getMe` y cargar el token fuera de Git en el servidor.
- [ ] Regenerar en BotFather el token compartido durante la puesta en marcha y
  reemplazarlo en `/etc/agora/agora.env` antes de habilitar producción.
- [x] Desactivar Privacy Mode para que el bot pueda incorporar mensajes del grupo.
- [x] Crear el grupo privado, agregar el bot y cargar fuera de Git el ID del grupo
  y el primer participante autorizado. Los otros cinco se incorporarán después.
- [x] Registrar y verificar `https://agora.maese.com.ar/webhooks/telegram` con secreto.
- [ ] Cargar ambos bloques de proveedor en `/etc/agora/agora.env`.
- [x] Desplegar inicialmente con `CHAT_PROVIDER=telegram`; readiness, health,
  migraciones y recepción real del webhook quedaron verificados.
- [ ] Alternar controladamente a WhatsApp y volver a Telegram, verificando jobs congelados.

## 6. Meta

- [x] Confirmar app `Agora` y Business Portfolio mediante `auth.json`.
- [x] Confirmar que el caso de uso WhatsApp está agregado.
- [x] Configurar categoría `Messaging`, ícono y URLs públicas legales de la app.
- [x] Verificar requisitos oficiales de Groups API al 17/07/2026.
- [x] Confirmar límite de participantes compatible.
- [x] Confirmar que Groups API no vincula una Community existente.
- [ ] Obtener elegibilidad Official Business Account. La WABA `Agora` tiene
  actividad desde febrero de 2026 y cumple los requisitos documentados: negocio
  verificado, nombre `Approved` y verificación en dos pasos activa. Sin embargo,
  `official_business_account.oba_status` devuelve `NOT_STARTED` y el botón
  `Submit request` continúa deshabilitado. Direct Support cerró el caso
  `28216915367901535`: OBA sólo está disponible por autoservicio cuando Meta
  habilita el botón o mediante un BSP con Meta Point of Contact; actualmente no
  la ofrece a las demás cuentas. El caso anterior de revisión del nombre
  `28334978916099204` figura `Resolved`.
- [x] Completar verificación del negocio y requisito de 2FA para sus usuarios
  (Business Portfolio y Tech Provider verificados; 2FA requerida para todos).
- [x] Activar la verificación en dos pasos específica del número. Graph API
  confirmó el registro del número de `Agora` con `success:true` y WhatsApp
  Manager muestra `Enabled` el 29/07/2026. El PIN continúa sólo en
  `/etc/agora/agora.env` (`640`, `root:deploy`).
- [x] Recuperar App Secret y cargarlo sin exponerlo; se validó con un webhook
  firmado por Meta en producción.
- [x] Registrar el número productivo de la WABA `Agora` en Cloud API. Graph API
  devuelve `code_verification_status=VERIFIED`, `name_status=APPROVED`,
  `platform_type=CLOUD_API` y throughput `STANDARD`; WhatsApp Manager muestra
  `Connected` el 29/07/2026.
- [x] Confirmar el acceso del system user y su token permanente a la WABA
  `Agora` y su número. El token sigue válido, `/me` responde y permitió
  registrar el número y suscribir la app el 29/07/2026.
- [x] Inventariar WABA ID y Phone Number ID fuera de Git, en
  `/etc/agora/agora.env`.
- [ ] Crear el grupo oficial por Groups API e invitar a los seis participantes.
  El intento real contra el número de `Agora` el 29/07/2026 devuelve `131215`
  porque el número todavía no es elegible para Groups API.
- [x] Configurar callback `https://agora.maese.com.ar/webhooks/whatsapp`.
- [x] Suscribir `messages`, `group_lifecycle_update`,
  `group_participants_update`, `group_settings_update` y
  `group_status_update`.
- [x] Configurar URLs de privacidad, términos y eliminación.
- [x] Reemplazar en el perfil empresarial la URL raíz que devolvía `404` por
  las páginas públicas existentes de privacidad y términos; la API confirmó
  `success` y devolvió ambas URLs el 18/07/2026.
- [x] Verificar challenge real y webhook firmado desde el panel de Meta; una
  segunda entrega idéntica fue deduplicada.
- [ ] Probar mensaje entrante, documento, respuesta y estados.
- [ ] Publicar la app después de elegibilidad OBA y revisión legal, pero antes
  del piloto real: Meta no entrega eventos productivos mientras está sin
  publicar.
- [x] Iniciar App Review, documentar el uso real de
  `whatsapp_business_messaging` y `whatsapp_business_management`, declarar
  responsables/procesadores y retirar `public_profile` de la solicitud porque
  Agora no lo usa. La entrega queda pendiente del screencast y las llamadas
  reales que sólo pueden ejecutarse después de habilitar Groups API.

## 7. Secretos y consentimiento

- [x] Cargar `OPENAI_API_KEY` directamente en `oracle` y validarla contra la API
  sin exponerla (`HTTP 200`).
- [x] Cargar App Secret, token permanente, WABA ID y Phone Number ID
  directamente en `oracle`.
- [ ] Cargar Group ID y `WHATSAPP_ALLOWED_USER_IDS` directamente en `oracle` cuando
  existan el grupo elegible y los consentimientos.
- [x] Preparar un formulario versionado de consentimiento sin datos personales.
- [x] Documentar consentimiento de los seis participantes fuera del proyecto
  (confirmado por el responsable el 18/07/2026).
- [x] Publicar política de privacidad propuesta.
- [x] Publicar términos y procedimiento de exportación/eliminación.
- [x] Revisar y aprobar legalmente los textos propuestos, incluido que el RAG
  cerrado y limitado de Agora no infringe las condiciones de Meta para
  proveedores o asistentes de IA (revisión y evidencia conservadas fuera del
  proyecto; confirmado por el responsable el 18/07/2026).

## 8. Prueba final

- [x] Firma inválida devuelve `401` en producción.
- [ ] Evento real se persiste una sola vez (el webhook de prueba firmado del
  panel de Meta ya demostró persistencia y deduplicación).
- [ ] Documento real queda en PostgreSQL, se extrae y se indexa.
- [ ] `@agora` responde dentro del grupo con citas.
- [x] `/agora` responde en Telegram como reply; el flujo real completó embeddings,
  generación y envío sin jobs muertos el 31/07/2026. Las citas se validarán con
  el primer documento del grupo.
- [x] Reiniciar el contenedor no pierde ni duplica jobs (prueba controlada en
  producción el 18/07/2026: mismo UUID pendiente antes y después del reinicio,
  una sola fila, `completed` en un intento, un fragmento generado y datos
  sintéticos eliminados; `/ready` volvió a `200`).
- [x] Un despliegue inválido vuelve al digest anterior (digest inexistente
  rechazado, rollback ejecutado y `/ready` continuó saludable).
- [x] Un backup local se restaura en una base aislada.
- [x] Merge de PR a `main` publica y despliega exactamente un digest (PR #11,
  run `29662623493`; el índice público, `.deployed-image` y el contenedor
  coinciden en
  `sha256:bd61017f3ac57cc95fc2f98ec895ecd53c95d915cdd3c2499dd5f9528a247a9d`).
- [x] Todos los participantes dieron consentimiento (evidencia conservada fuera
  del proyecto; confirmado por el responsable el 18/07/2026).

Agora estará completo cuando no quede ninguna casilla abierta y la evidencia
externa confirme Meta, GitHub, `oracle`, OpenAI y el flujo real.
