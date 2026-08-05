DO
$$
    BEGIN
        PERFORM cron.schedule(
                'nightly-idempotency-cleanup',
                '0 0 * * *',
                'DELETE FROM idempotency WHERE expire_at < NOW()'
                );
    EXCEPTION
        WHEN undefined_function OR undefined_table OR invalid_schema_name THEN
            RAISE WARNING 'pg_cron is not available on this Postgres instance; skipping nightly-idempotency-cleanup cron job. Relying on the application-level cleanup worker instead.';
    END;
$$;
