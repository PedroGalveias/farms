FROM postgres:18.4-alpine3.24

ENV POSTGRES_DB "postgres"
ENV POSTGRES_USER "postgres"
ENV POSTGRES_PASSWORD "password"

RUN apk add --no-cache postgresql-pg_cron && \
    cp /usr/lib/postgresql18/pg_cron.so /usr/local/lib/postgresql/ && \
    cp /usr/share/postgresql18/extension/* /usr/local/share/postgresql/extension

COPY --chmod=755 setup_pg_cron.sh /docker-entrypoint-initdb.d/
