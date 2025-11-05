#!/bin/bash
set -e

# Wait for Postgres to be ready
until pg_isready -h localhost -p 5432 -U postgres; do
  echo "Waiting for Postgres to be ready..."
  sleep 1
done

# Check if tables already exist (first-run detection)
TABLE_EXISTS=$(psql -h localhost -p 5432 -U postgres -d postgres -tAc "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'sessions');")

if [ "$TABLE_EXISTS" = "t" ]; then
  echo "Tables already exist, skipping DDL execution"
  exit 0
fi

echo "First run detected, executing DDL..."

# Execute DDL
psql -h localhost -p 5432 -U postgres -d postgres -f /docker-entrypoint-initdb.d/schema.sql

echo "DDL execution completed"
