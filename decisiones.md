# Decisiones de Agora

Última actualización: 31 de julio de 2026.

Este documento es autoritativo para la versión 1 y prevalece sobre propuestas
anteriores del roadmap.

## Producto

- Agora representa un único espacio lógico cerrado mediante Telegram o
  WhatsApp. Telegram es el proveedor predeterminado y sólo uno está activo por
  instancia.
- Se usarán sólo Telegram Bot API y WhatsApp Cloud/Groups API oficiales. No hay
  fallback a chats 1:1, Signal ni automatización de WhatsApp Web.
- El bot busca conocimiento y responde cuando se lo invoca con `@agora_telegram_bot` o
  `/agora` en Telegram y `@agora` en WhatsApp.
- Cada proveedor tiene grupo y allowlist propios; los identificadores no se
  versionan. Ambos comparten exclusivamente `KNOWLEDGE_SPACE_ID`.
- No existe sitio, interfaz web, login ni API pública de búsqueda.
- Idioma único: español.
- Contenido v1: texto, DOC, DOCX, PDF, XLS y XLSX.
- No se importa historial.
- Volumen esperado: bajo.
- Los mensajes, texto extraído y archivos originales se conservan mientras el
  proyecto esté activo o hasta una solicitud válida de eliminación.

## Proveedores de chat

- `CHAT_PROVIDER` acepta `telegram` o `whatsapp`, usa Telegram por defecto y
  requiere reiniciar el contenedor al cambiar.
- Ambos webhooks permanecen registrados. El proveedor inactivo se autentica y
  responde `200`, pero no persiste eventos.
- Eventos, mensajes, adjuntos, jobs y salidas registran proveedor. Los eventos
  y jobs inactivos quedan congelados y nunca se despachan con el otro cliente.
- Telegram admite grupos y supergrupos, usa secreto de webhook, limita descargas
  a 20 MiB, responde con `sendMessage` y no posee estados delivered/read.
- WhatsApp conserva su HMAC, límite de 25 MiB y estados sent/delivered/read.
- La búsqueda RAG se aísla por espacio lógico, no por grupo técnico, para
  mantener continuidad al alternar plataformas.

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
- El token permanente cargado en `/etc/agora/agora.env` sigue siendo válido,
  su sujeto está inventariado en `META_SYSTEM_USER_ID` y tiene acceso a la WABA
  productiva `Agora` y su número. `Rent` es otro proyecto y queda fuera del
  alcance de Agora.
- El callback productivo está verificado, la app está vinculada a la WABA y
  `messages` más los cuatro eventos grupales están suscritos en `v25.0`.
- La WABA y el negocio están aprobados. El 29/07/2026 el número de `Agora`
  quedó registrado en Cloud API: el nombre figura `Approved`, WhatsApp Manager
  muestra `Connected` y Graph API informa throughput `STANDARD`. El intento
  real de crear el grupo volvió a devolver `131215` el 30/07/2026 porque el
  número todavía no es elegible para Groups API.
- El perfil empresarial enlaza directamente a `/privacy` y `/terms`; se retiró
  la URL raíz porque devuelve `404` y podía perjudicar la validación externa
  del nombre comercial.
- Meta exige para OBA que el negocio lleve al menos 30 días registrado en
  WhatsApp Business Platform, tenga negocio verificado, nombre aprobado y
  verificación en dos pasos en el número. La WABA `Agora` tiene actividad desde
  febrero de 2026 y cumple esos requisitos, pero WhatsApp Manager todavía no
  habilita la solicitud y Graph API todavía informa
  `oba_status=NOT_STARTED` el 30/07/2026. Direct Support cerró el caso
  `28216915367901535` el 29/07/2026 indicando que OBA sólo está disponible por
  autoservicio cuando Meta habilita el botón o mediante un BSP con Meta Point of
  Contact, y que actualmente no la ofrece a las demás cuentas.
- La 2FA obligatoria para los usuarios del Business Portfolio no equivale a la
  verificación en dos pasos del número. Graph API confirmó el registro con el
  PIN y WhatsApp Manager muestra `Enabled` para el número de `Agora` el
  29/07/2026. El PIN definitivo se conserva únicamente en
  `/etc/agora/agora.env` con permisos `640 root:deploy`.
- La revisión del nombre de Direct Support `28334978916099204` figura
  `Resolved` y WhatsApp Manager muestra el nombre `Approved` al 29/07/2026.
  La solicitud OBA sigue deshabilitada y el panel pide volver a intentar más
  adelante.
- Meta no entrega webhooks productivos mientras la app permanece sin publicar.
  Por eso la publicación debe ocurrir después de la elegibilidad y la revisión
  legal, pero antes de ejecutar el piloto real.
- App Review está iniciado con sólo `whatsapp_business_messaging` y
  `whatsapp_business_management`; `public_profile` se retiró porque Agora no lo
  utiliza. Las descripciones y el formulario de tratamiento de datos quedaron
  preparados; el borrador figura `Not submitted` el 30/07/2026 y faltan el
  screencast y las llamadas reales dependientes del grupo.
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
- Telegram y Meta son procesadores posibles de mensajería; sólo el seleccionado
  recibe y envía contenido de Agora en una instancia determinada.

## Privacidad

- Responsable: Andres Castiglia.
- Contacto: `acastiglia@gmail.com`.
- Operación y residencia de participantes: Argentina.
- Todos los participantes deben consentir el tratamiento antes del piloto.
- Los seis participantes dieron consentimiento antes del piloto; la evidencia
  y la revisión legal se conservan fuera del proyecto.
- Telegram o Meta reciben la mensajería según el proveedor activo y OpenAI
  recibe el contenido necesario para embeddings y respuestas.
- Existen avisos públicos en `/privacy`, `/terms` y `/data-deletion`.
- Las solicitudes de acceso, exportación o eliminación se reciben por correo y
  requieren verificación de identidad.

## Pendientes que no pueden inventarse

- Rotación del token de Telegram compartido durante la puesta en marcha e
  incorporación de los cinco participantes restantes a la allowlist.
- Group ID y allowlist de participantes. WABA ID, Phone Number ID, App Secret y
  token permanente ya están cargados fuera de Git.
- Aprobación OBA/Groups API. El nombre, el negocio y la verificación en dos
  pasos del número ya están aprobados.
