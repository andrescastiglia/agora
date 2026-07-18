# Agent.md — Agora

## 1. Propósito

Agora es una memoria empresarial consultable que captura conocimiento compartido en una comunidad o grupos cerrados de WhatsApp, procesa mensajes de texto, audio y documentos, y permite recuperar ese conocimiento mediante búsqueda semántica y respuestas contextualizadas.

El proyecto debe priorizar, en este orden:

1. Utilidad real como memoria empresarial.
2. Privacidad, control de acceso y trazabilidad.
3. Bajo costo operativo.
4. Simplicidad arquitectónica y operativa.
5. Calidad de recuperación y respuestas.
6. Escalabilidad basada en evidencia, no anticipada.

Este archivo gobierna todo el ciclo de vida del proyecto: descubrimiento, diseño, desarrollo, pruebas, seguridad, despliegue, operación, observabilidad, documentación y evolución.

---

## 2. Regla principal para agentes

No inventar requisitos, restricciones, integraciones, capacidades de proveedores ni decisiones de producto.

Cuando una decisión material no esté definida:

1. Revisar primero el repositorio, documentación, issues y decisiones existentes.
2. Identificar con precisión qué dato falta y qué impacto tiene.
3. Preguntar antes de implementar una suposición irreversible o costosa.
4. Cuando sea posible continuar sin bloquear, usar una interfaz desacoplada, una configuración explícita o un valor pendiente claramente marcado.
5. Registrar las decisiones confirmadas en documentación versionada.

No preguntar por detalles menores que puedan resolverse con una opción segura, reversible, convencional y de bajo costo. Toda suposición menor debe quedar explícita en el cambio o en su documentación.

---

## 3. Alcance funcional inicial

El flujo objetivo es:

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
          Rust + Worker
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

Capacidades iniciales:

- Recibir y validar eventos provenientes de WhatsApp.
- Persistir el evento original antes de procesarlo.
- Procesar texto, audio y documentos de forma asíncrona.
- Transcribir audio mediante un proveedor configurable, inicialmente OpenAI cuando sea confirmado.
- Extraer texto de tipos de documento explícitamente soportados.
- Normalizar contenido y metadatos sin perder trazabilidad hacia la fuente.
- Dividir contenido en fragmentos adecuados para recuperación.
- Generar embeddings mediante un proveedor y modelo configurables.
- Almacenar datos relacionales y vectores en PostgreSQL con pgvector.
- Consultar mediante búsqueda semántica, filtros y recuperación híbrida cuando esté justificada.
- Responder desde una API, un chat web y eventualmente un bot de WhatsApp.
- Citar siempre las fuentes internas usadas para construir una respuesta.

Fuera de alcance hasta confirmación explícita:

- Automatizaciones que actúen en nombre de usuarios.
- Edición o eliminación de mensajes en WhatsApp.
- Integraciones distintas de WhatsApp, OpenAI y PostgreSQL.
- Entrenamiento o fine-tuning de modelos.
- Arquitectura multi-región.
- Microservicios independientes por tipo de contenido.
- Kubernetes.
- Infraestructura de streaming compleja.
- Funciones de vigilancia, evaluación de empleados o perfilado personal.

---

## 4. Restricciones confirmadas

- Lenguaje principal: Rust.
- Base de datos: PostgreSQL con pgvector.
- Canal de origen inicial: WhatsApp Cloud API.
- Objetivo principal: memoria empresarial consultable.
- Alcance de gobierno de este archivo: ciclo completo del proyecto.
- Restricción crítica: costo operativo bajo.

No reemplazar estas decisiones sin una decisión arquitectónica explícita.

---

## 5. Principios de arquitectura

### 5.1 Monolito modular primero

Comenzar con un monolito modular en Rust y procesos desplegables mínimos:

- API/webhook.
- Worker asíncrono.
- PostgreSQL.

API y worker pueden compartir el mismo workspace, dominio, librerías y artefacto base. Separar servicios únicamente cuando exista evidencia de aislamiento operativo, seguridad, escalado o despliegue independiente.

### 5.2 PostgreSQL como núcleo operativo

Mientras el volumen lo permita, PostgreSQL debe cubrir:

- Datos de dominio.
- Estado de procesamiento.
- Cola de trabajos persistente o patrón equivalente.
- Idempotencia.
- Auditoría.
- Índices de búsqueda textual.
- Vectores con pgvector.

No incorporar Redis, Kafka, RabbitMQ, Elasticsearch u otra infraestructura sin justificar por qué PostgreSQL ya no satisface el requisito medido.

### 5.3 Procesamiento asíncrono e idempotente

El webhook debe responder rápidamente después de:

- Validar autenticidad y estructura mínima.
- Registrar el evento de manera durable.
- Detectar duplicados.
- Encolar o marcar el trabajo para procesamiento.

Todo trabajo debe ser reintentable e idempotente. Los reintentos no pueden duplicar mensajes, documentos, fragmentos ni embeddings.

### 5.4 Proveedores detrás de interfaces

Las integraciones externas deben depender de traits o puertos del dominio:

- WhatsApp.
- Descarga de medios.
- Transcripción.
- Extracción documental.
- Embeddings.
- Generación de respuestas.
- Almacenamiento de objetos, si se incorpora.

La selección de proveedor, modelo, límites y timeouts debe ser configurable.

### 5.5 Recuperación antes que generación

La calidad de Agora depende primero de la ingestión, normalización, permisos, recuperación y citas. No compensar una recuperación deficiente aumentando el tamaño o costo del modelo generativo.

---

## 6. Organización sugerida del código

Hasta que el repositorio defina otra estructura, usar un Cargo workspace con módulos de responsabilidad clara:

```text
/
├── Cargo.toml
├── crates/
│   ├── agora-domain/
│   ├── agora-application/
│   ├── agora-infrastructure/
│   ├── agora-api/
│   └── agora-worker/
├── migrations/
├── docs/
│   ├── architecture/
│   ├── decisions/
│   ├── operations/
│   └── product/
├── tests/
├── docker/
├── .github/workflows/
└── Agent.md
```

Esta estructura es una base reversible, no una obligación de crear crates vacíos. Crear un crate nuevo solo cuando tenga una frontera real y dependencias coherentes.

Reglas de dependencia:

- `domain` no depende de infraestructura ni frameworks web.
- `application` orquesta casos de uso y depende del dominio.
- `infrastructure` implementa persistencia y proveedores externos.
- `api` adapta HTTP/webhooks a casos de uso.
- `worker` ejecuta trabajos asíncronos mediante los mismos casos de uso.

---

## 7. Modelo de datos y trazabilidad

Cada contenido debe conservar una cadena verificable:

```text
organización
  → espacio/comunidad/grupo
    → conversación
      → mensaje fuente
        → recurso adjunto
          → texto extraído o transcripción
            → versión normalizada
              → fragmentos
                → embeddings
```

Requisitos mínimos:

- Identificadores internos estables.
- Identificadores externos cuando existan.
- Hash de contenido para deduplicación.
- Estado y versión del procesamiento.
- Fecha del evento original y fecha de ingestión.
- Autor o remitente representado de forma compatible con privacidad.
- Origen, grupo y contexto de conversación.
- Referencia exacta desde cada fragmento hacia su fuente.
- Modelo y versión usados para transcripción, embeddings y generación.
- Registro de errores y reintentos.
- Estado de retención o eliminación.

No sobrescribir silenciosamente resultados derivados. Cuando cambie un modelo o algoritmo, versionar el resultado y permitir reprocesamiento controlado.

---

## 8. Seguridad y privacidad

La información capturada puede ser confidencial. Aplicar privacidad y seguridad desde el diseño.

Obligatorio:

- Verificar la autenticidad de webhooks según el mecanismo oficial vigente.
- No registrar secretos, tokens, contenido completo ni datos personales innecesarios en logs.
- Cifrar comunicaciones en tránsito.
- Mantener secretos fuera del código y de imágenes de contenedor.
- Aplicar mínimo privilegio a base de datos, infraestructura y proveedores.
- Aislar datos por organización o tenant desde el modelo de dominio.
- Propagar permisos de acceso desde el origen hasta consultas y respuestas.
- Impedir que una búsqueda recupere contenido no autorizado.
- Registrar accesos y operaciones administrativas relevantes.
- Definir eliminación y retención antes de manejar datos reales.
- Sanitizar documentos y limitar tamaño, tipo, tiempo de procesamiento y expansión.
- Tratar texto, documentos y mensajes como entrada no confiable, incluida la inyección de instrucciones.

Nunca incluir secretos ni contenido empresarial real en fixtures públicos, errores, trazas o prompts de prueba.

Cualquier funcionalidad que afecte consentimiento, retención, exportación, eliminación, residencia de datos o identidad requiere decisión explícita antes de producción.

---

## 9. Control de acceso

La autorización no debe limitarse al acceso general a la aplicación.

Cada consulta debe evaluar al menos:

- Organización.
- Usuario o identidad solicitante.
- Espacios, comunidades o grupos permitidos.
- Tipo de contenido y restricciones adicionales.
- Estado de retención o eliminación.

Los filtros de autorización deben aplicarse dentro de la recuperación, antes de enviar contexto a un modelo externo.

No usar únicamente instrucciones de prompt como mecanismo de seguridad.

---

## 10. Estrategia de costos

Toda decisión debe considerar costo por organización, mensaje, minuto de audio, documento, embedding, consulta y almacenamiento.

Preferencias:

- Procesar una sola vez y reutilizar resultados.
- Deduplicar antes de invocar servicios pagos.
- Usar modelos económicos que cumplan la calidad requerida.
- Permitir batching cuando reduzca costo sin aumentar riesgo.
- Limitar tamaño de archivos, duración de audio, contexto y cantidad de fragmentos.
- Evitar regenerar embeddings si el contenido y modelo no cambiaron.
- Aplicar caché solo con una métrica y una política de invalidación claras.
- Almacenar el mínimo necesario de medios originales, sujeto a retención y auditoría.
- Escalar verticalmente y simplificar operación antes de distribuir componentes.
- Incorporar presupuestos, cuotas y alertas por tenant antes de abrir uso amplio.

Todo PR que agregue una llamada paga o almacenamiento significativo debe describir:

- Unidad de costo.
- Frecuencia esperada.
- Límites.
- Estrategia de deduplicación.
- Comportamiento ante agotamiento de presupuesto.

---

## 11. Fiabilidad

Diseñar para entrega repetida, eventos fuera de orden y fallos parciales.

Requisitos:

- Claves de idempotencia para eventos y trabajos.
- Estados explícitos de procesamiento.
- Reintentos con backoff y límite.
- Cola de fallos o estado terminal inspeccionable.
- Timeouts en toda operación externa.
- Cancelación y cierre ordenado.
- Transacciones en fronteras consistentes.
- Migraciones hacia adelante y estrategia de rollback compatible.
- Herramientas para reprocesar por mensaje, documento, grupo, rango temporal o versión de pipeline.

No confirmar procesamiento exitoso si solo se recibió el webhook.

---

## 12. Observabilidad

Usar logs estructurados y correlacionados mediante identificadores de evento, mensaje, trabajo, tenant y solicitud.

Métricas mínimas:

- Eventos recibidos, aceptados, duplicados y rechazados.
- Latencia del webhook.
- Trabajos pendientes, activos, exitosos, reintentados y fallidos.
- Tiempo por etapa del pipeline.
- Minutos de audio y páginas/documentos procesados.
- Tokens o unidades facturables por proveedor.
- Costo estimado por tenant y operación.
- Cantidad de fragmentos y embeddings.
- Latencia de consulta y generación.
- Recuperaciones sin resultados.
- Errores por proveedor y tipo.

Evitar sistemas de observabilidad costosos al inicio. Preferir estándares abiertos y una configuración mínima que pueda evolucionar.

---

## 13. Calidad de búsqueda y respuestas

Toda respuesta basada en memoria empresarial debe:

- Diferenciar hechos recuperados de inferencias.
- Mostrar referencias legibles a mensajes o documentos fuente.
- Indicar cuando no existe evidencia suficiente.
- Evitar afirmar información que no aparece en las fuentes autorizadas.
- Mantener el contexto temporal del conocimiento.
- Priorizar versiones más recientes sin ocultar contradicciones relevantes.

Evaluar recuperación con un conjunto versionado de preguntas y fuentes esperadas. Medir, como mínimo:

- Recall de documentos o fragmentos relevantes.
- Precisión de citas.
- Respuestas sin evidencia.
- Latencia.
- Costo por consulta.

No optimizar prompts sin conservar casos de evaluación reproducibles.

---

## 14. Desarrollo en Rust

Usar la edición estable de Rust definida en el repositorio.

Estándares mínimos:

- `cargo fmt` sin diferencias.
- `cargo clippy` sin advertencias nuevas, salvo excepciones justificadas.
- `cargo test` exitoso.
- Errores tipados en librerías y contexto suficiente en fronteras operativas.
- Sin `unwrap`, `expect` o `panic` en caminos de producción salvo invariantes documentadas.
- Tipos de dominio para identificadores y estados importantes.
- Serialización explícita y compatible hacia atrás en contratos persistidos o públicos.
- Dependencias mínimas, mantenidas y justificadas.
- Código asíncrono sin bloquear el runtime.
- Límites explícitos para concurrencia, memoria y tamaño de entrada.

No introducir `unsafe` sin una justificación documentada, pruebas específicas y revisión explícita.

---

## 15. Pruebas

Aplicar una pirámide práctica:

- Pruebas unitarias para dominio, normalización, fragmentación y políticas.
- Pruebas de integración para PostgreSQL, migraciones y repositorios.
- Pruebas de contrato para payloads externos y proveedores.
- Pruebas end-to-end para los caminos críticos de ingestión y consulta.
- Fixtures anonimizados y pequeños.

Casos obligatorios:

- Evento duplicado.
- Evento fuera de orden.
- Firma inválida.
- Medio no disponible.
- Audio o documento demasiado grande.
- Tipo de archivo no soportado.
- Fallo y timeout de proveedor.
- Reintento después de persistencia parcial.
- Eliminación o revocación de acceso.
- Consulta que intenta cruzar tenants o grupos.
- Respuesta sin fuentes suficientes.

No depender de APIs pagas reales en la suite normal de CI.

---

## 16. API y contratos

- Versionar endpoints públicos o mantener compatibilidad demostrable.
- Usar esquemas explícitos para solicitudes, respuestas y eventos.
- Validar tamaño, formato y contenido en el borde.
- Utilizar errores consistentes sin filtrar datos internos.
- Documentar idempotencia, autenticación, autorización y límites.
- Paginar colecciones.
- Evitar exponer directamente el esquema de persistencia.

Los webhooks deben estar aislados de la lógica de negocio mediante adaptadores y casos de uso.

---

## 17. Migraciones y datos

- Toda modificación de esquema se realiza mediante migración versionada.
- Una migración debe poder ejecutarse de manera segura en entornos existentes.
- Evitar bloqueos prolongados y reescrituras masivas sin estrategia.
- Separar cambios de esquema de backfills costosos.
- Probar migraciones desde una versión anterior representativa.
- No eliminar columnas o datos hasta completar una ventana de compatibilidad definida.
- Documentar operaciones de recuperación y backup antes de producción.

Los datos derivados deben poder reconstruirse desde fuentes retenidas o declarar claramente cuando no sea posible.

---

## 18. Entrega y despliegue

Antes de producción deben existir:

- Construcción reproducible.
- Imagen de contenedor mínima y ejecutada sin privilegios.
- Health checks separados para vida y disponibilidad.
- Migraciones controladas.
- Configuración por entorno.
- Backups verificados de PostgreSQL.
- Procedimiento de rollback.
- Límites de recursos.
- Despliegue de staging o entorno equivalente.
- Runbooks para fallos críticos.

La infraestructura inicial debe favorecer un proveedor sencillo, servicios administrados mínimos y costos previsibles. La elección concreta del proveedor de hosting queda pendiente de confirmación.

---

## 19. CI/CD

Todo cambio debe pasar, como mínimo:

1. Formato.
2. Lint.
3. Compilación.
4. Pruebas unitarias.
5. Pruebas de integración aplicables.
6. Verificación de migraciones.
7. Análisis de dependencias y secretos.
8. Construcción del artefacto o imagen.

No desplegar automáticamente a producción desde una rama no protegida. La estrategia exacta de ramas, revisiones y promoción queda pendiente de definición.

---

## 20. Documentación y decisiones

Mantener junto al código:

- Visión y alcance del producto.
- Diagramas de contexto y contenedores.
- Modelo de datos.
- Contratos externos.
- Runbooks.
- Política de seguridad y privacidad.
- Presupuesto y modelo de costos.
- ADRs para decisiones significativas.

Crear un ADR cuando una decisión:

- Sea difícil de revertir.
- Agregue infraestructura o un proveedor.
- Cambie fronteras de seguridad o privacidad.
- Modifique contratos públicos.
- Aumente significativamente costo u operación.
- Reemplace una restricción confirmada de este archivo.

---

## 21. Flujo de trabajo para cambios

Antes de implementar:

1. Leer este archivo y la documentación relevante.
2. Inspeccionar código, migraciones, tests e issues relacionados.
3. Declarar el objetivo y criterios de aceptación.
4. Identificar riesgos de privacidad, seguridad y costo.
5. Preguntar cualquier decisión material no resuelta.

Durante la implementación:

1. Mantener el cambio pequeño y coherente.
2. Evitar refactors no relacionados.
3. Agregar pruebas junto con el comportamiento.
4. Actualizar contratos, migraciones y documentación.
5. Mantener compatibilidad o documentar la ruptura.

Antes de finalizar:

1. Ejecutar formato, lint y pruebas.
2. Revisar logs y errores para evitar filtraciones.
3. Verificar idempotencia y aislamiento entre tenants.
4. Evaluar impacto de costo.
5. Resumir cambios, decisiones, riesgos y pendientes.

---

## 22. Criterio de terminado

Una funcionalidad no está terminada solo porque compila.

Debe cumplir:

- Criterios de aceptación verificables.
- Pruebas relevantes.
- Manejo de errores y reintentos.
- Seguridad y autorización.
- Observabilidad suficiente.
- Documentación del comportamiento.
- Migraciones seguras cuando correspondan.
- Evaluación de costo operativo.
- Estrategia de despliegue y rollback proporcional al riesgo.

---

## 23. Decisiones abiertas que requieren confirmación

No asumir respuestas para los siguientes puntos:

1. Nombre definitivo del producto y terminología de dominio.
2. Viabilidad y modalidad exacta de captura desde comunidades o grupos mediante las capacidades oficiales vigentes de WhatsApp.
3. Modelo de consentimiento de participantes.
4. Política de retención y eliminación de mensajes, medios, transcripciones y documentos.
5. Modelo de tenants: una empresa, múltiples empresas o instalación por empresa.
6. Identidad, autenticación y roles del chat web.
7. Forma de mapear permisos de WhatsApp a permisos de consulta.
8. Tipos y límites de documentos soportados inicialmente.
9. Idiomas principales y requisitos de transcripción.
10. Proveedor y modelo de embeddings.
11. Proveedor y modelo de generación de respuestas.
12. Estrategia de almacenamiento de medios originales.
13. Proveedor y región de infraestructura.
14. Presupuesto mensual y límites por tenant.
15. Requisitos legales, regulatorios y de residencia de datos.
16. SLA, RPO y RTO esperados.
17. Alcance exacto del MVP y métricas de éxito.
18. Estrategia de ramas, revisiones y releases.

Cuando una tarea dependa de uno de estos puntos, detener esa decisión específica y solicitar confirmación antes de cerrar la implementación.

---

## 24. Antipatrones prohibidos

- Inventar soporte de una API externa.
- Procesar antes de persistir el evento original.
- Confiar en entrega única de webhooks.
- Mezclar datos entre organizaciones o grupos.
- Enviar contenido no autorizado a un modelo externo.
- Guardar secretos o contenido sensible en logs.
- Crear microservicios sin necesidad medida.
- Añadir infraestructura para resolver un problema hipotético.
- Usar prompts como sustituto de autorización.
- Responder sin fuentes cuando la respuesta afirma conocimiento interno.
- Reprocesar contenido pago sin deduplicación o control de versión.
- Introducir una dependencia sin evaluar mantenimiento, licencia, seguridad y costo.
- Cambiar arquitectura, proveedor o modelo de datos sin documentar la decisión.

---

## 25. Directiva final

Construir Agora como un sistema confiable de memoria empresarial, no como una demostración de chatbot.

Cada cambio debe mejorar al menos una de estas dimensiones sin degradar silenciosamente las demás:

- Captura confiable.
- Trazabilidad.
- Recuperación relevante.
- Privacidad y autorización.
- Respuestas sustentadas.
- Operación simple.
- Costo controlado.

Ante conflicto entre sofisticación y simplicidad, elegir la solución más simple que satisfaga requisitos confirmados y pueda medirse en producción.