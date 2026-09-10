#!/usr/bin/env bash
# Control the local Postgres test instance.
#
#   ./scripts/db.sh up      start it and wait until it accepts connections
#   ./scripts/db.sh reset   restore the seed data without restarting
#   ./scripts/db.sh down    stop and remove it, including its volume
#   ./scripts/db.sh psql    open a shell against it
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
  up)
    docker compose up -d --wait
    echo "Postgres ready on localhost:55432 (database mydb_test)"
    ;;
  reset)
    docker compose exec -T postgres \
      psql -v ON_ERROR_STOP=1 -U mydb_test -d mydb_test < testing/seed.sql > /dev/null
    echo "Seed data restored"
    ;;
  down)
    docker compose down -v
    ;;
  psql)
    docker compose exec postgres psql -U mydb_test -d mydb_test
    ;;
  *)
    sed -n '2,8p' "$0"
    exit 1
    ;;
esac
