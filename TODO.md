# TODO — Pendientes de Agora

Última revisión: 1 de septiembre de 2026.

## Despliegue

- [ ] Publicar el próximo tag `vX.X.X` sobre `main` y verificar en `oracle` la
  migración `0011`, `/ready` y los procedimientos de derechos de datos.

## Telegram

- [ ] Regenerar en BotFather el token expuesto durante la puesta en marcha,
  actualizarlo en `oracle` y volver a verificar identidad y webhook.
- [ ] Incorporar a los cinco participantes restantes al grupo piloto y a
  `TELEGRAM_ALLOWED_USER_IDS`.
- [ ] Validar con un documento real la persistencia binaria y del hash, la
  extracción, el indexado, la deduplicación y una respuesta con citas.

## WhatsApp y Meta

- [ ] Obtener OBA y elegibilidad para Groups API; crear el grupo oficial,
  invitar a los seis participantes y configurar su ID y allowlist en `oracle`.
- [ ] Completar las llamadas y el screencast de App Review, enviar la revisión
  y obtener acceso avanzado para `whatsapp_business_management` y
  `whatsapp_business_messaging`.
- [ ] Probar el flujo real completo de WhatsApp: entrada y deduplicación,
  documento, respuesta con citas y estados salientes.
- [ ] Alternar controladamente Telegram → WhatsApp → Telegram y comprobar
  aislamiento, congelamiento y reanudación de eventos y jobs.
