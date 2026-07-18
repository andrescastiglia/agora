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
- Los documentos originales se guardan en un bucket privado de OCI Object
  Storage mediante su endpoint S3 compatible.
- No se contrata un servicio externo de alertas.

## Privacidad

- Responsable: Andres Castiglia.
- Contacto: `acastiglia@gmail.com`.
- Operación y residencia de participantes: Argentina.
- Todos los participantes deben consentir el tratamiento antes del piloto.
- Meta recibe mensajería y OpenAI recibe el contenido necesario para embeddings
  y respuestas.
- Existen avisos públicos en `/privacy`, `/terms` y `/data-deletion`.
- Las solicitudes de acceso, exportación o eliminación se reciben por correo y
  requieren verificación de identidad.

## Pendientes que no pueden inventarse

- App Secret y contraseña de reautenticación de Meta.
- Token permanente del system user y Group ID. WABA ID y Phone Number ID ya
  están inventariados fuera de Git.
- Aprobación OBA/Groups API y verificación del negocio.
- Namespace, región, bucket y Customer Secret Key de OCI Object Storage.
- Consentimiento documentado de los seis participantes.
