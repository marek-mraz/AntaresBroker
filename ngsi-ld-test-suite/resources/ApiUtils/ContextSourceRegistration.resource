*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         OperatingSystem
Library         JSONLibrary
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}    csourceRegistrations

${CONTENT_TYPE_JSON}                            application/json
${CONTENT_TYPE_LD_JSON}                         application/ld+json

${ERROR_TYPE_BAD_REQUEST_DATA}                  https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_INVALID_REQUEST}                   https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}                https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                                     ${EMPTY}
${context_source_url}                           http://${context_source_host}:${context_source_port}


*** Keywords ***
Prepare Context Source Registration From File
    [Arguments]
    ...    ${id}
    ...    ${registration_file}
    ...    ${entity_id}=${EMPTY}
    ...    ${entity_id_pattern}=${EMPTY}
    ...    ${mode}=${EMPTY}
    ...    ${endpoint}=${EMPTY}
    ...    ${broker_url}=${EMPTY}
    ...    ${operations}=[]
    ${payload}=    Load JSON From File    ${EXECDIR}/data/${registration_file}
    ${final_context_source_url}=    Set Variable If    '${broker_url}' != ''    ${broker_url}    ${context_source_url}
    ${registration_payload}=    Update Value To JSON
    ...    ${payload}
    ...    $.id
    ...    ${id}
    ${registration_payload}=    Update Value To JSON
    ...    ${registration_payload}
    ...    $..endpoint
    ...    ${final_context_source_url}${endpoint}
    IF    '${entity_id}' != ''
        ${dict}=    Create Dictionary    id=${entity_id}
        ${registration_payload}=    Add Object To JSON
        ...    ${registration_payload}
        ...    $..entities[*]
        ...    ${dict}
    END
    IF    '${entity_id_pattern}' != ''
        ${dict}=    Create Dictionary    idPattern=${entity_id_pattern}
        ${registration_payload}=    Add Object To JSON
        ...    ${registration_payload}
        ...    $..entities[*]
        ...    ${dict}
    END
    IF    '${mode}' != ''
        ${dict}=    Create Dictionary    mode=${mode}
        ${registration_payload}=    Add Object To JSON
        ...    ${registration_payload}
        ...    $
        ...    ${dict}
    END
    IF    ${operations}
        ${dict}=    Create Dictionary    operations=${operations}
        ${registration_payload}=    Add Object To JSON
        ...    ${registration_payload}
        ...    $
        ...    ${dict}
    END
    RETURN    ${registration_payload}

Create Context Source Registration
    [Tags]    actor_context-producer
    [Arguments]    ${context_source_registration_payload}

    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    json=${context_source_registration_payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Context Source Registration
    RETURN    ${response}

Create Context Source Registration With Return
    [Tags]    actor_context-producer
    [Arguments]    ${payload}    ${content_type}=${CONTENT_TYPE_LD_JSON}    ${context}=${EMPTY}    ${accept}=${EMPTY}    ${broker_url}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    ${final_url}=    Set Variable If    '${broker_url}' != ''    ${broker_url}    ${url}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept=${context}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    POST
    ...    url=${final_url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Context Source Registration
    RETURN    ${response}

Delete Context Source Registration
    [Tags]    actor_context-producer
    [Arguments]    ${context_source_registration_id}    ${broker_url}=${EMPTY}

    ${final_url}=    Set Variable If    '${broker_url}' != ''    ${broker_url}    ${url}
    ${response}=    DELETE    url=${final_url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${context_source_registration_id}
    Output    ${response}    Delete Context Source Registration
    RETURN    ${response}

Delete Context Source Registration With Return
    [Tags]    actor_context-producer
    [Arguments]    ${registration_id}    ${broker_url}=${EMPTY}
    ${final_url}=    Set Variable If    '${broker_url}' != ''    ${broker_url}    ${url}
    ${response}=    DELETE
    ...    url=${final_url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${registration_id}
    ...    expected_status=any
    Output    ${response}    Delete Context Source Registration
    RETURN    ${response}

Update Context Source Registration
    [Tags]    actor_context-producer
    [Arguments]    ${context_source_registration_id}    ${update_fragment}

    ${response}=    PATCH
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${context_source_registration_id}
    ...    json=${update_fragment}
    ...    expected_status=any
    Output    ${response}    Update Context Source Registration
    RETURN    ${response}

Update Context Source Registration From File
    [Tags]    actor_context-producer
    [Arguments]    ${context_source_registration_id}    ${filename}    ${content_type}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}

    ${file_content}=    Get File    ${EXECDIR}/data/csourceRegistrations/${filename}
    ${response}=    PATCH
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${context_source_registration_id}
    ...    data=${file_content}
    ...    headers=${headers}
    ...    expected_status=any

    Output    ${response}    Update Context Source Registration From File
    RETURN    ${response}

Update Context Source Registration With Return
    [Tags]    actor_context-producer
    [Arguments]    ${registration_id}    ${fragment}    ${content_type}    ${accept}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    ${response}=    PATCH
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${registration_id}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Update Context Source Registration
    RETURN    ${response}
