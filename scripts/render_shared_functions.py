import os
import subprocess
from pathlib import Path

import dotenv
import requests


def load_dotenv():
    print('Loading Dotenv')
    dotenv_file = dotenv.find_dotenv()

    if not dotenv_file:
        dotenv_file = os.path.join(os.getcwd(), '.env')
        Path(dotenv_file).touch()

    dotenv.load_dotenv(dotenv_file)
    return dotenv_file


def send_get_request(url, headers):
    print(f'Sending GET request to: {url}')
    try:
        r = requests.get(url, headers=headers)
        r.raise_for_status()
        return r
    except requests.exceptions.RequestException as e:
        print("HTTP Error")
        print(e.args[0])
        exit(1)


def send_post_request(url, headers, body=None):
    print(f'Sending POST request to: {url}')
    try:
        r = requests.post(url, headers=headers, json=body)
        r.raise_for_status()
        return r
    except requests.exceptions.RequestException as e:
        print("HTTP Error")
        print(e.args[0])
        exit(1)


def send_put_request(url, headers, body):
    print(f'Sending PUT request to: {url}')
    try:
        r = requests.put(url, headers=headers, json=body)
        r.raise_for_status()
        return r
    except requests.exceptions.RequestException as e:
        print("HTTP Error")
        print(e.args[0])
        exit(1)


def send_delete_request(url, headers):
    print(f'Sending DELETE request to: {url}')
    try:
        r = requests.delete(url, headers=headers)
        r.raise_for_status()
        return r
    except requests.exceptions.RequestException as e:
        print("HTTP Error")
        print(e.args[0])
        exit(1)


def fetch_render_db_id(render_api_base_url, base_headers):
    print('Fetching Database Id')
    request_url = f'{render_api_base_url}/postgres?includeReplicas=true&limit=20'
    response = send_get_request(request_url, base_headers)
    return response.json()[0]['postgres']['id']


def fetch_render_db_connection_info(render_db_id, render_api_base_url, base_headers):
    print('Fetching Database Connection Info')
    request_url = f'{render_api_base_url}/postgres/{render_db_id}/connection-info'
    response = send_get_request(request_url, base_headers)
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
