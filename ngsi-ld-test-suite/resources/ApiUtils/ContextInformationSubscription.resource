*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         JSONLibrary
Library         OperatingSystem
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${SUBSCRIPTION_ENDPOINT_PATH}               subscriptions/

${CONTENT_TYPE_JSON}                        application/json
${CONTENT_TYPE_LD_JSON}                     application/ld+json

${SUBSCRIPTION_ID_PREFIX}                   urn:ngsi-ld:Subscription:

${ERROR_TYPE_BAD_REQUEST_DATA}              https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_INVALID_REQUEST}               https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}            https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound
${ERROR_TYPE_ALREADY_EXISTS}                https://uri.etsi.org/ngsi-ld/errors/AlreadyExists
${ERROR_TYPE_LD_CONTEXT_NOT_AVAILABLE}      https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable

${response}                                 ${EMPTY}


*** Keywords ***
Create Subscription
    [Tags]    actor_context-subscriber
    [Arguments]
    ...    ${subscription_id}
    ...    ${filename_path}
    ...    ${content_type}
    ...    ${accept}=${EMPTY}
    ...    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${filename_path}
    ${subscription}=    Update Value To JSON    ${subscription_payload}    $.id    ${subscription_id}
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    json=${subscription}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Subscription
    RETURN    ${response}

Create Subscription From File
    [Tags]    actor_context-subscriber
    [Arguments]    ${filename}
    ${file_content}=    Get File    ${EXECDIR}/data/subscriptions/${filename}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    data=${file_content}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Subscription From File
    RETURN    ${response}

Create Subscription From Subscription Payload
    [Tags]    actor_context-subscriber
    [Arguments]
    ...    ${subscription_payload}
    ...    ${content_type}=${CONTENT_TYPE_LD_JSON}
    ...    ${accept}=${EMPTY}
    ...    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    POST
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    json=${subscription_payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Subscription From Subscription Payload
    RETURN    ${response}

Delete Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}
    ${response}=    DELETE
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${subscription_id}
    ...    expected_status=any
    Output    ${response}    Delete Subscription
    RETURN    ${response}

Query Subscriptions
    [Tags]    actor_context-subscriber
    [Arguments]    ${context}=${EMPTY}    ${limit}=${EMPTY}    ${offset}=${EMPTY}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    '${offset}'!=''    Set To Dictionary    ${params}    offset=${offset}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END

    ${response}=    GET
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Subscriptions
    RETURN    ${response}

Retrieve Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${id}    ${accept}=${EMPTY}    ${context}=${EMPTY}    ${content_type}=${CONTENT_TYPE_JSON}

    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END

    ${response}=    GET
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Subscription
    RETURN    ${response}

Update Subscription
    [Tags]    actor_context-subscriber
    [Arguments]
    ...    ${subscription_id}
    ...    ${fragment_filename}
    ...    ${content_type}
    ...    ${accept}=${EMPTY}
    ...    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${subscription_update_fragment}=    Load JSON From File    ${EXECDIR}/data/${fragment_filename}
    ${response}=    PATCH
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${subscription_id}
    ...    json=${subscription_update_fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Update Subscription
    RETURN    ${response}

Update Subscription With Payload
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}    ${payload}    ${content_type}    ${accept}=${EMPTY}    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    PATCH
    ...    url=${url}/${SUBSCRIPTION_ENDPOINT_PATH}${subscription_id}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Update Subscription With Payload
    RETURN    ${response}
