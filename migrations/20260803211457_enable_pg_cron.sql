DO
$$
    BEGIN
        CREATE EXTENSION IF NOT EXISTS pg_cron;
    EXCEPTION
        WHEN feature_not_supported THEN
            RAISE WARNING 'pg_cron extension is not available on this Postgres instance; skipping. Relying on the application-level cleanup worker instead.';
    END;
$$;
