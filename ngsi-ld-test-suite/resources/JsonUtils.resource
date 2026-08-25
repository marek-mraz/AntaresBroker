*** Settings ***
Library         String
Library         JSONLibrary
Variables       ${EXECDIR}/resources/variables.py


*** Variables ***
${notification_server_url}=     http://${notification_server_host}:${notification_server_port}/notify
${context_source_url}=          http://${context_source_host}:${context_source_port}


*** Keywords ***
Load Entity
    [Arguments]    ${entity_file_name}    ${entity_id}

    ${entity}=    Load Test Sample    entities/${entity_file_name}    ${entity_id}
    RETURN    ${entity}

Load Test Sample
    [Arguments]    ${test_sample_file_path}    ${test_sample_id}=${EMPTY}

    ${test_sample}=    Load JSON From File    ${EXECDIR}/data/${test_sample_file_path}
    IF    '${test_sample_id}'!=''
        ${test_sample}=    Update Value To JSON    ${test_sample}    $.id    ${test_sample_id}
    END

    RETURN    ${test_sample}

Load Entity As Serialized Array
    [Arguments]    ${entity_file_name}    ${entity_id}

    ${entity}=    Load Test Sample    entities/${entity_file_name}    ${entity_id}
    ${entity_as_list}=    Create List    ${entity}
    ${serialized_entity}=    Convert JSON To String    ${entity_as_list}
    RETURN    ${serialized_entity}

Load Subscription Sample With Reachable Endpoint
    [Arguments]
    ...    ${subscription_file_path}
    ...    ${subscription_id}=${EMPTY}
    ...    ${notification_endpoint_uri}=${notification_server_url}

    ${subscription_payload}=    Load Test Sample    ${subscription_file_path}    ${subscription_id}
    ${subscription}=    Update Value To JSON
    ...    ${subscription_payload}
    ...    $..notification['endpoint']['uri']
    ...    ${notification_endpoint_uri}
    RETURN    ${subscription}

Set Entity Id In Subscription
    [Arguments]    ${subscription_payload}    ${entity_id}
    ${dict}=    Create Dictionary    id=${entity_id}
    ${subscription}=    Add Object To JSON    ${subscription_payload}    $.entities[0]    ${dict}
    RETURN    ${subscription}

Load Context Source Registration Sample With Reachable Context Source
    [Arguments]
    ...    ${context_source_registration_file_path}
    ...    ${context_source_registration_id}=${EMPTY}
    ...    ${context_source_endpoint_uri}=${context_source_url}

    ${context_source_registration_payload}=    Load Test Sample
    ...    ${context_source_registration_file_path}
    ...    ${context_source_registration_id}
    ${context_source_registration}=    Update Value To JSON
    ...    ${context_source_registration_payload}
    ...    $.endpoint
    ...    ${context_source_endpoint_uri}
    RETURN    ${context_source_registration}

Create Batch Operation Result
    [Arguments]    ${success}    ${errors}

    &{batch_operation_result}=    Create Dictionary    success=${success}    errors=${errors}
    RETURN    &{batch_operation_result}

Create Update Result
    [Arguments]    ${updated}    ${notUpdated}

    &{update_result}=    Create Dictionary    updated=${updated}    notUpdated=${notUpdated}
    RETURN    &{update_result}

Upsert Element In Entity
    [Arguments]    ${initial_entity}    ${fragment}    ${jsonPath}=$

    ${updated_entity}=    Add Object To JSON    ${initial_entity}    ${jsonPath}    ${fragment}
    RETURN    ${updated_entity}

Create Empty Array Result
    ${empty_list}=    Create List
    ${empty_response}=    Convert JSON To String    ${empty_list}
    RETURN    ${empty_response}
