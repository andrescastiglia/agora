# Decisiones de Agora

Última actualización: 18 de julio de 2026.

Este documento es autoritativo para la versión 1 y prevalece sobre propuestas
anteriores del roadmap.

## Producto

- Agora funciona únicamente dentro de un grupo cerrado asociado al proyecto
  Agora en WhatsApp.
- Se usarán sólo WhatsApp Cloud API y Groups API oficiales. No hay fallback a
  chats 1:1 ni automatización de WhatsApp Web.
- El bot busca conocimiento y responde automáticamente cuando se lo invoca con
  `@agora`.
- Los seis participantes iniciales se configuran mediante
  `ALLOWED_WHATSAPP_IDS`; sus números no se versionan.
- No existe sitio, interfaz web, login ni API pública de búsqueda.
- Idioma único: español.
- Contenido v1: texto, DOC, DOCX, PDF, XLS y XLSX.
- No se importa historial.
- Volumen esperado: bajo.
- Los mensajes, texto extraído y archivos originales se conservan mientras el
  proyecto esté activo o hasta una solicitud válida de eliminación.

## Restricción confirmada de Meta

La documentación oficial de Groups API, verificada el 17 de julio de 2026,
indica:

- la empresa debe tener Official Business Account;
- un grupo admite hasta ocho participantes, incluido el número empresarial;
- el grupo se crea programáticamente y por invitación;
- no se convierte ni vincula una Community de consumidor ya existente.

Los seis participantes más el número empresarial caben en el límite. El
lanzamiento sigue condicionado a obtener elegibilidad OBA y aceptar que el
recurso técnico será el grupo creado por Groups API. No se adoptará un fallback
1:1.

## Meta

- Business Portfolio: `Andres Castiglia`.
- App de Meta Developers: `Agora`.
- El número productivo está definido fuera de Git.
- `auth.json` local permite administración mediante Playwright, pero no reemplaza
  secretos productivos ni el reingreso de contraseña que Meta pueda exigir.
- App ID, Business ID, WABA ID y Phone Number ID pueden inventariarse como
  configuración. App Secret, tokens y PIN nunca se envían por chat ni Git.
- El system user `agora` tiene un token permanente válido (`expires_at=0`) con
  acceso a la WABA y al número; App Secret, token e IDs productivos están
  cargados en `/etc/agora/agora.env`.
- El callback productivo está verificado, la app está vinculada a la WABA y
  `messages` más los cuatro eventos grupales están suscritos en `v25.0`.
- La WABA está aprobada y el negocio verificado, pero el número todavía no es
  elegible para Groups API: al 18/07/2026 la API devuelve `131215`, OBA figura
  `NOT_STARTED`, el nombre vigente está `DECLINED` y existe un nuevo nombre en
  `PENDING_REVIEW`.
- El perfil empresarial enlaza directamente a `/privacy` y `/terms`; se retiró
  la URL raíz porque devuelve `404` y podía perjudicar la validación externa
  del nombre comercial.
- Meta exige para OBA que el número lleve al menos 30 días registrado, tenga
  2FA, negocio verificado y nombre aprobado. WhatsApp Manager todavía informa
  que la solicitud OBA no está disponible. Su registro de actividad comienza
  el 08/07/2026 y las solicitudes de verificación de nombre aparecen desde el
  09/07/2026; por prudencia, el siguiente intento de OBA debe hacerse desde el
  08/08/2026.
- La 2FA obligatoria para los usuarios del Business Portfolio no equivale a la
  verificación en dos pasos del número. Esta última sigue desactivada y Meta
  devolvió `Unknown error` al intentar establecer y confirmar un PIN nuevo. Se
  abrió Direct Support `27824698277217409`; el PIN temporal se eliminó al
  fallar la activación y no se guardó en Git ni producción.
- La revisión del nombre está escalada por Direct Support
  `28334978916099204`: el nombre figura simultáneamente rechazado y con una
  revisión pendiente, lo que impide presentar `Agora by Andres Castiglia`.
- Meta no entrega webhooks productivos mientras la app permanece sin publicar.
  Por eso la publicación debe ocurrir después de la elegibilidad y la revisión
  legal, pero antes de ejecutar el piloto real.
- App Review está iniciado con sólo `whatsapp_business_messaging` y
  `whatsapp_business_management`; `public_profile` se retiró porque Agora no lo
  utiliza. Las descripciones y el formulario de tratamiento de datos quedaron
  preparados; faltan el screencast y las llamadas reales dependientes del
  grupo.
- Agora es un RAG de dominio limitado al conocimiento del grupo, no un asistente
  general abierto. La revisión legal previa al piloto debe confirmar que esta
  caracterización cumple las condiciones vigentes de Meta para servicios de IA.

## Infraestructura

- Dominio: `agora.maese.com.ar`.
- Servidor: alias SSH `oracle`, Ubuntu ARM64.
- Nginx y Certbot terminan TLS.
- La API escucha sólo en `127.0.0.1:8088`.
- PostgreSQL 17 y pgvector escuchan sólo en localhost.
- Los servicios existentes de `oracle` deben preservarse.
- La aplicación se ejecuta en Docker Compose bajo el usuario `deploy`; PM2 de
  otros proyectos no se modifica.
- No se guardan backups fuera de `oracle`, por decisión del responsable. Esta
  decisión reduce la capacidad de recuperación ante pérdida total de la VM.

## GitHub y despliegue

- Repositorio: `andrescastiglia/agora`, público.
- Imagen GHCR pública.
- `main` cambia mediante PR y checks obligatorios.
- No se exige aprobación humana del PR.
- Cada merge a `main` despliega inmediatamente en el environment `oracle`, sin
  aprobación manual.
- El deploy usa el digest inmutable ARM64/AMD64, readiness y rollback automático.

## Proveedores

- OpenAI es el único proveedor de IA:
  - generación: `gpt-5.6-sol`, reasoning effort `medium`;
  - embeddings: `text-embedding-3-small`, 1536 dimensiones;
  - Responses API con almacenamiento desactivado.
- Presupuesto mensual esperado: bajo.
- Los documentos originales se guardan como `BYTEA` en PostgreSQL, junto con
  su hash SHA-256.
- Los backups de PostgreSQL incluyen también los documentos originales, por lo
  que crecerán con el volumen documental.
- No se contrata un servicio externo de alertas.

## Privacidad

- Responsable: Andres Castiglia.
- Contacto: `acastiglia@gmail.com`.
- Operación y residencia de participantes: Argentina.
- Todos los participantes deben consentir el tratamiento antes del piloto.
- Los seis participantes dieron consentimiento antes del piloto; la evidencia
  y la revisión legal se conservan fuera del proyecto.
- Meta recibe mensajería y OpenAI recibe el contenido necesario para embeddings
  y respuestas.
- Existen avisos públicos en `/privacy`, `/terms` y `/data-deletion`.
- Las solicitudes de acceso, exportación o eliminación se reciben por correo y
  requieren verificación de identidad.

## Pendientes que no pueden inventarse

- Group ID y allowlist de participantes. WABA ID, Phone Number ID, App Secret y
  token permanente ya están cargados fuera de Git.
- Aprobación OBA/Groups API, activación de la verificación en dos pasos del
  número y resolución de la revisión del nombre. El negocio ya está verificado;
  ambos problemas del número están escalados en Direct Support.
