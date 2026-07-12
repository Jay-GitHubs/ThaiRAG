# Backup sidecar: postgres client tools + curl (Qdrant snapshot API) + bash +
# tzdata (fire at local wall-clock time). Runs scripts/docker-backup-loop.sh.
#
# Why a container and not host cron: macOS TCC blocks cron's access to
# /Volumes even with a granted-and-restarted Full Disk Access entry (lived
# failure, 2026-07-13 — two denials post-grant). Docker's daemon already has
# authorized access to the project volume, so scheduling INSIDE the stack is
# immune to TCC and to macOS updates revoking grants.
FROM postgres:16-alpine
RUN apk add --no-cache bash curl tzdata
COPY scripts/docker-backup-loop.sh /backup-loop.sh
ENTRYPOINT ["bash", "/backup-loop.sh"]
