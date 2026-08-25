url = 'http://scorpio1:9090/ngsi-ld/v1'
temporal_api_url = 'http://scorpio1:9090/ngsi-ld/v1'
ngsild_test_suite_context = 'https://raw.githubusercontent.com/marek-mraz/ngsi-ld-test-suite/main/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld'
notification_server_host = '172.28.0.18'
notification_server_port = 8085
mqtt_broker_host = 'ngsi-ld-test-suite-mosquitto-container'
mqtt_broker_non_default_port = 8085
context_source_host = '172.28.0.18'
context_source_port = 8086
context_server_host = '172.28.0.18'
context_server_port = 8087
core_context = 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld'

# whether the Temporal Representation of an Entity should be deleted when an Entity is deleted
delete_temporal_on_core_delete = True

# GitHub repository details
# github_owner = 'your_github_username'
# github_broker_repo = 'context_broker_repository'
# github_token = 'your_github_token'
