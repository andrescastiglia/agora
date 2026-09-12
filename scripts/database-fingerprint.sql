\set ON_ERROR_STOP on
SET timezone = 'UTC';
SELECT 'extensions', string_agg(extname || '=' || extversion, ',' ORDER BY extname) FROM pg_extension;
SELECT format('SELECT %L, count(*), md5(coalesce(string_agg(md5(to_jsonb(t)::text), %L ORDER BY md5(to_jsonb(t)::text)), %L)) FROM %I.%I t;', tablename, '', '', schemaname, tablename)
FROM pg_tables WHERE schemaname='public' ORDER BY tablename
\gexec
SELECT 'invalid_indexes', count(*) FROM pg_index WHERE NOT indisvalid;
SELECT 'non_agora_table_owners', count(*) FROM pg_tables WHERE schemaname='public' AND tableowner <> 'agora';
