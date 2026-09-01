# Procedimiento de acceso, corrección, exportación y eliminación

Este procedimiento se ejecuta únicamente después de verificar la identidad de
la persona solicitante y registrar la solicitud fuera de Git. Los identificadores
personales y los archivos exportados nunca se incorporan al repositorio.

## Acceso y exportación

1. Confirmar el proveedor y el identificador usado en el grupo.
2. Ejecutar en `oracle` como `root`:

   ```bash
   /opt/agora/export-participant-data.sh \
     telegram IDENTIFICADOR /ruta/protegida/export.json
   # o usar whatsapp como proveedor
   ```

3. Revisar el JSON, entregarlo por un canal acordado y eliminar la copia local
   cuando se confirme la recepción. El archivo se crea con permisos `0600` e
   incluye mensajes, adjuntos originales en Base64, chunks, jobs, respuestas y
   cualquier webhook pendiente asociado.

Las correcciones se realizan sobre el registro identificado, se documentan y
se vuelven a exportar para validación. No se modifican migraciones ni evidencia
de auditoría.

## Eliminación o retiro

1. Retirar a la persona del grupo o de la allowlist correspondiente y
   redesplegar antes de borrar, para impedir una reingesta posterior.
2. Si corresponde entregar una copia, exportarla antes de continuar.
3. Ejecutar como `root`:

   ```bash
   /opt/agora/delete-participant-data.sh \
     --confirm --replace-backups telegram IDENTIFICADOR
   ```

La operación elimina mensajes, adjuntos, chunks, jobs y respuestas asociados;
minimiza payloads de webhooks todavía pendientes; genera un backup nuevo y
destruye los backups cifrados anteriores. El log
`/var/log/agora-data-deletions.log` conserva fecha, proveedor y conteos por tipo
de objeto, pero ningún identificador ni derivación del identificador.

## Restauraciones

Después de una eliminación sólo puede restaurarse el backup generado por esa
misma operación o uno posterior. Si una restauración excepcional parte de una
copia más antigua conservada fuera del procedimiento, la aplicación debe
mantenerse detenida y la eliminación debe repetirse antes de volver a aceptar
webhooks.

## Verificación

Los SQL usados por ambos comandos se prueban contra PostgreSQL/pgvector en
`tests/repository_integration.rs`. También deben verificarse `bash -n`, los
permisos del archivo exportado, el log sin identificadores y una restauración
del backup de reemplazo en una base aislada.
