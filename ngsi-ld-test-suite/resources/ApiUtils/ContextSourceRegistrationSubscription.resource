*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         OperatingSystem
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}       csourceSubscriptions

${CONTENT_TYPE_JSON}                                            application/json
${CONTENT_TYPE_LD_JSON}                                         application/ld+json

${ERROR_TYPE_ALREADY_EXISTS}                                    https://uri.etsi.org/ngsi-ld/errors/AlreadyExists
${ERROR_TYPE_BAD_REQUEST_DATA}                                  https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_INVALID_REQUEST}                                   https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}                                https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                                                     ${EMPTY}


*** Keywords ***
Create Context Source Registration Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_payload}    ${accept}=${EMPTY}    ${content_type}=${CONTENT_TYPE_LD_JSON}

    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    ${response}=    POST
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}
    ...    json=${subscription_payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Context Source Registration Subscription
    RETURN    ${response}

Delete Context Source Registration Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}

    ${response}=    DELETE
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}/${subscription_id}
    ...    expected_status=any
    Output    ${response}    Delete Context Source Registration Subscription
    RETURN    ${response}

Query Context Source Registration Subscriptions
    [Tags]    actor_context-subscriber
    [Arguments]    ${context}=${EMPTY}    ${limit}=${EMPTY}    ${offset}=${EMPTY}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    '${offset}'!=''    Set To Dictionary    ${params}    offset=${offset}

    ${response}=    GET
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Context Source Registration Subscriptions
    RETURN    ${response}

Retrieve Context Source Registration Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}    ${context}=${EMPTY}    ${accept}=${CONTENT_TYPE_JSON}

    &{headers}=    Create Dictionary
    Set To Dictionary    ${headers}    Accept    ${accept}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    GET
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}/${subscription_id}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Context Source Registration Subscription
    RETURN    ${response}

Update Context Source Registration Subscription
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}    ${subscription_update_fragment}    ${context}=${ngsild_test_suite_context}

    &{headers}=    Create Dictionary
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary    ${headers}    Link    ${context_link}
    END
    ${response}=    PATCH
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}/${subscription_id}
    ...    json=${subscription_update_fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Update Context Source Registration Subscription
    RETURN    ${response}

Update Context Source Registration Subscription From File
    [Tags]    actor_context-subscriber
    [Arguments]    ${subscription_id}    ${file_path}
    ${file_content}=    Get File    ${EXECDIR}/data/${file_path}
    ${response}=    PATCH
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_SUBSCRIPTION_ENDPOINT_PATH}/${subscription_id}
    ...    data=${file_content}
    ...    expected_status=any
    Output    ${response}    Update Context Source Registration Subscription From File
    RETURN    ${response}
