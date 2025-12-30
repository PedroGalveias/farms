import os
import subprocess

import dotenv
import requests


def fetch_render_db_id(render_api_base_url, base_headers):
    print('Fetching Database Id')
    request_url = f'{render_api_base_url}/postgres?includeReplicas=true&limit=20'
    response = requests.get(request_url, headers=base_headers)
    assert response.status_code == 200
    return response.json()[0]['postgres']['id']


def fetch_render_db_connection_info(render_db_id, render_api_base_url, base_headers):
    print('Fetching Database Connection Info')
    request_url = f'{render_api_base_url}/postgres/{render_db_id}/connection-info'
    response = requests.get(request_url, headers=base_headers)
    assert response.status_code == 200
    # {
    #   "password": "string",
    #   "internalConnectionString": "string",
    #   "externalConnectionString": "string",
    #   "psqlCommand": "string"
    # }
    return response.json()


def store_in_dotenv(var_key, var_value, dotenv_file):
    print(f"Storing '{var_key}' in dotenv file.")
    os.environ[var_key] = var_value
    dotenv.set_key(dotenv_file, var_key, var_value)


def store_database_connection_in_dotenv(db_connection_string, dotenv_file):
    print('Storing Database Connection string')
    store_in_dotenv('DATABASE_URL', db_connection_string, dotenv_file)


def migrate_render_db():
    print('Migrating Database')
    try:
        result = subprocess.run(
            ['sqlx', 'migrate', 'run'],
            capture_output=True,
            text=True,
            check=True
        )
        print('Migration successful')
        print('Output:', result.stdout)
        if result.stderr:
            print('Stderr:', result.stderr)
    except subprocess.CalledProcessError as e:
        print(f'Migration failed with exit code {e.returncode}')
        print("Output:", e.stdout)
        print("Error:", e.stderr)
