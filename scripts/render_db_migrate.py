from render_shared_functions import *

dotenv_file = dotenv.find_dotenv()
dotenv.load_dotenv(dotenv_file)

render_api_base_url = os.getenv('RENDER_API_BASE_URL').rstrip('/')
render_api_key = os.getenv('RENDER_API_KEY')

base_headers = {
    'accept': 'application/json',
    'authorization': f'Bearer {render_api_key}'
}

render_db_id = fetch_render_db_id(render_api_base_url, base_headers)
render_db_connection_info = fetch_render_db_connection_info(
    render_db_id,
    render_api_base_url,
    base_headers
)
store_database_connection_in_dotenv(render_db_connection_info['externalConnectionString'], dotenv_file)
migrate_render_db()
