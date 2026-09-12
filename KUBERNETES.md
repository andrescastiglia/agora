# Operación de Agora en Kubernetes

La migración autorizada usa K3s en `oracle`, con una réplica de Agora y un
StatefulSet PostgreSQL independiente. Nginx/Certbot y los demás proyectos
permanecen en el host. No hay HA: el PV y los backups residen en el mismo disco.

## Infraestructura

- Clúster existente reutilizado: K3s `v1.36.4+k3s1`, nodo `oracle`, etcd embebido.
- API privada, Traefik/ServiceLB deshabilitados, pods `10.42.0.0/16`, services
  `10.43.0.0/16`. kube-proxy iptables con NodePorts exclusivamente en loopback.
- Swap del host conservada; kubelet `failSwapOn=false`, `NoSwap`.
- `k8s/bootstrap.yaml` contiene namespace, PV estático y RBAC. Solo root lo aplica.
- `k8s/database` contiene PostgreSQL, servicios internos y política de red.
  La imagen fijada incluye PostgreSQL 17.10 y pgvector 0.8.5; la fuente era 17.9.
- PV `/var/lib/agora-k8s/postgres`, 20 GiB declarados, RWO y Retain. Esa capacidad
  no impone una cuota física. PVC `data-postgres-0` se conserva al eliminar o
  escalar el StatefulSet. Nunca montar el directorio compartido del host.
- `k8s/app` contiene únicamente recursos de aplicación. Nginx conecta a
  `127.0.0.1:30088`. PostgreSQL usa `postgres.agora.svc.cluster.local:5432`.
- La aplicación sigue ejecutando migraciones al arrancar. El corte no agrega ni
  modifica migraciones. Una sola réplica y Recreate evitan solapar workers.

## Secretos y releases

La fuente protegida continúa en `/etc/agora/agora.env`. Ejecutar como root
`configure-k8s-secrets.py` para sincronizar únicamente claves necesarias y la
contraseña independiente del usuario de aplicación. Los cambios de entorno
requieren reiniciar el Deployment. El administrador de PostgreSQL es separado.
No imprimir el archivo, Secrets o kubeconfigs.

`/etc/agora/runtime.conf` selecciona explícitamente `compose` o `kubernetes` y
la base activa. Está protegido `640 root:deploy`; los scripts no deben elegir
otra base como fallback después de un error.

Los tags `vX.X.X` sobre el último `main` continúan publicando imagen multiarch y
desplegando el digest. El usuario de release usa
`/etc/agora/kubeconfig-deploy`; carece de permiso para modificar StatefulSets,
PVC/PV, Secrets o namespaces. Revisar/renovar su certificado antes del vencimiento.
La instalación inicial de recursos la realiza root; los releases solo actualizan
recursos ya existentes de `k8s/app`.

`deploy-oracle.sh` restaura explícitamente la imagen anterior ante errores de
aplicación, timeout o readiness pública. Este rollback no revierte esquemas SQL.
Las futuras migraciones deben ser compatibles con la imagen anterior.

## Corte y rollback de datos

El script root `migrate-oracle-k8s.sh` utiliza un lock compartido con deploy.
Requiere el preflight reciente en `/var/lib/agora-migration/preflight-ok`, generado
solo después de revisar 15 minutos de carga, memoria y al menos 30 GiB libres.

Antes de ejecutarlo, instalar el upstream Nginx `agora_backend` y el control
`/etc/nginx/agora-webhooks-maintenance`: solo los webhooks devuelven 503 cuando el
archivo existe. Las páginas legales permanecen disponibles.

1. Ensayar dump/restore en `agora_rehearsal` con la API detenida para esa copia.
2. Comparar `database-fingerprint.sql`: todas las tablas, extensiones e índices.
3. Ejecutar `migrate-oracle-k8s.sh cutover` como root. Si falla, mantener el
   mantenimiento y revisar checkpoints; no repetir ciegamente el comando.
4. El corte bloquea entradas, drena el proveedor activo, detiene Compose, bloquea
   el rol antiguo, cifra un dump final, restaura y compara antes de activar Agora.
5. Probar backup/restauración y observar antes de retirar la fuente antigua.

`migrate-oracle-k8s.sh rollback` detiene Kubernetes. Antes de intentar activar el Deployment,
recupera Compose sobre la fuente intacta. Desde `target.activated` siempre hace dump de
la base nueva y restaura una base distinta en el host, compara y actualiza el
endpoint de Compose. Si la base nueva está inaccesible, se detiene: no acepta
pérdida de datos volviendo silenciosamente a la copia antigua.

Las copias de ensayo y migración contienen datos reales: mantenerlas protegidas
y eliminarlas junto con backups obsoletos ante solicitudes válidas de supresión.
No borrar la base antigua hasta cumplidos siete días y verificadas estabilidad
y restauración. Cualquier rollback reinicia esta revisión de copias retenidas.

## Backups y aceptación

Los timers systemd conservan backups cifrados locales con retención de 14 días.
`backup-postgres.sh` usa la base activa; `test-restore-postgres.sh` restaura en una
base temporal única y la elimina al terminar. `backup-k3s.sh` cifra un snapshot
etcd, token y configuración. La passphrase permanece fuera de Git.

Validar restauración, hashes de originales y documentos sintéticos, entrega
idempotente, SIGTERM, caída del worker, recreación del pod/PVC, rollback de
release, rollback de datos y puertos privados. Las pruebas de proveedores usan
mocks y no envían mensajes a grupos reales. Mantener observación 48 horas y
verificar los servicios ajenos antes y después.

El worker deja de tomar trabajos al apagar y espera hasta 300 segundos; el pod
concede 330. Si agota el plazo, los claims siguen en PostgreSQL y se recuperan
tras su lease de 15 minutos. `sending` y `delivery_unknown` nunca se reenvían
solos. `/ready` también depende del worker; no valida credenciales con APIs externas.

Después del corte, `install-k8s-operations.sh` instala y habilita observaciones
cada cinco minutos, backup diario del clúster y revisión diaria del retiro de la
base antigua. El retiro requiere siete días, 48 horas de observaciones exitosas
sin huecos mayores a diez minutos y una restauración recién probada. Ante cualquier
fallo conserva la fuente; no fuerza la eliminación. Los resultados se guardan en
`/var/log/agora-k8s-observation.jsonl` y `/var/lib/agora-migration/legacy.retired`.

Para reproducir la preparación desde un checkout confiable, ejecutar como root
`scripts/bootstrap-oracle-k8s.sh` después de instalar K3s: crea namespace/PV,
provisiona primero el Secret administrador protegido, arranca PostgreSQL y
configura el rol de aplicación. También instala las herramientas operativas.
El comando de corte vuelve a instalar esas herramientas antes de seleccionar
Kubernetes, garantizando que el timer ejecute el backup actualizado.
