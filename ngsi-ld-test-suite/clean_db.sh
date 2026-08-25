#!/bin/bash
# Truncate all NGSI-LD data tables across every schema (tenant-safe), in one DO block.
# Never drop/recreate the database — only truncate data tables (schema + Flyway functions stay).
# Target container is configurable so this works for the single dev broker (scorpio-dev-postgres)
# AND broker-1 of the 5-broker stack (scorpio-postgres-1): set CLEAN_DB_CONTAINER.
sudo docker exec "${CLEAN_DB_CONTAINER:-scorpio-dev-postgres}" psql -U ngb -d ngb -q -c "
DO \$\$ DECLARE r RECORD; BEGIN
  FOR r IN SELECT schemaname, tablename FROM pg_tables
    WHERE tablename IN ('entity','temporalentity','temporalentityattrinstance','entitymap',
                        'csource','csourceinformation','subscriptions','registry_subscriptions') LOOP
    EXECUTE 'TRUNCATE TABLE '||quote_ident(r.schemaname)||'.'||quote_ident(r.tablename)||' CASCADE';
  END LOOP;
END \$\$;" 2>&1 | grep -v '^$'
