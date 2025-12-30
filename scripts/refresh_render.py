import time

from render_shared_functions import *

dotenv_file = load_dotenv()

render_api_base_url = os.getenv('RENDER_API_BASE_URL').rstrip('/')
render_api_key = os.getenv('RENDER_API_KEY')
render_service_id = os.getenv('RENDER_API_SERVICE_ID')
render_owner_id = os.getenv('RENDER_OWNER_ID')
render_environment_id = os.getenv('RENDER_ENVIRONMENT_ID')

render_service_base_url = 'https://farms-0ivm.onrender.com'
base_headers = {
    'accept': 'application/json',
    'authorization': f'Bearer {render_api_key}'
}


def delete_render_db(render_db_id):
    print('Deleting render db')
    request_url = f'{render_api_base_url}/postgres/{render_db_id}'
    send_delete_request(request_url, base_headers.copy())


def create_new_render_db():
    print('Creating new render db')
    request_url = f'{render_api_base_url}/postgres'
    headers = base_headers.copy()
    headers.update({'Content-Type': 'application/json'})
    body = {
        'databaseName': 'farms',
        'databaseUser': 'farmer',
        'enableHighAvailability': False,
        'plan': 'free',
        'enableDiskAutoscaling': False,
        'ipAllowList': [
            {
                'cidrBlock': '0.0.0.0/0',
                'description': 'everywhere'
            }
        ],
        'version': '18',
        'name': 'farms-db',
        'environmentId': render_environment_id,
        'region': 'frankfurt',
        'ownerId': render_owner_id
    }
    response = send_post_request(request_url, headers, body)
    return response.json()


def update_render_service_env_variable(env_var_key, env_var_value):
    print(f"Updating '{env_var_key}' environment variable")
    request_url = f'{render_api_base_url}/services/{render_service_id}/env-vars/{env_var_key}'
    headers = base_headers.copy()
    headers.update({'Content-Type': 'application/json'})
    body = {
        'value': env_var_value
    }
    send_put_request(request_url, headers, body)


def trigger_render_service_restart():
    print('Triggering render service restart')
    request_url = f'{render_api_base_url}/services/{render_service_id}/restart'
    send_post_request(request_url, base_headers.copy())


def test_render_service():
    print('Testing render service')
    request_url = f'{render_service_base_url}/health_check'
    response = send_get_request(request_url, headers=None)
    assert response.status_code == 200

    request_url = f'{render_api_base_url}/farms'
    response = send_get_request(request_url, headers=None)
    assert response.status_code == 200


existing_render_db_id = fetch_render_db_id(render_api_base_url, base_headers)
delete_render_db(existing_render_db_id)

new_render_db = create_new_render_db()
print('Sleeping for 20s')
time.sleep(20)

new_render_db_connection_info = fetch_render_db_connection_info(
    new_render_db['id'],
    render_api_base_url,
    base_headers
)

store_database_connection_in_dotenv(new_render_db_connection_info['externalConnectionString'], dotenv_file)
migrate_render_db()

# update_render_service_env_variable(
#    'DATABASE_URL',
#    new_render_db_connection_info['internalConnectionString']
# )
update_render_service_env_variable(
    'APP_DATABASE__DATABASE_NAME',
    new_render_db['databaseName']
)
update_render_service_env_variable(
    'APP_DATABASE__HOST',
    new_render_db['id']
)
update_render_service_env_variable(
    'APP_DATABASE__PASSWORD',
    new_render_db_connection_info['password']
)

trigger_render_service_restart()
print('Sleeping for 5s')
time.sleep(5)
test_render_service()
