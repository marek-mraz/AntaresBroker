*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}    csourceRegistrations

${ERROR_TYPE_BAD_REQUEST_DATA}                  https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_RESOURCE_NOT_FOUND}                https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                                     ${EMPTY}


*** Keywords ***
Query Context Source Registrations
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${context}=${EMPTY}
    ...    ${id}=${EMPTY}
    ...    ${type}=${EMPTY}
    ...    ${idPattern}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${q}=${EMPTY}
    ...    ${csf}=${EMPTY}
    ...    ${georel}=${EMPTY}
    ...    ${geometry}=${EMPTY}
    ...    ${coordinates}=${EMPTY}
    ...    ${geoproperty}=${EMPTY}
    ...    ${timeproperty}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${limit}=${EMPTY}
    ...    ${offset}=${EMPTY}
    ...    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    '${id}'!=''    Set To Dictionary    ${params}    id=${id}
    IF    '${type}'!=''    Set To Dictionary    ${params}    type=${type}
    IF    '${idPattern}'!=''
        Set To Dictionary    ${params}    idPattern=${idPattern}
    END
    IF    '${attrs}'!=''    Set To Dictionary    ${params}    attrs=${attrs}
    IF    '${q}'!=''    Set To Dictionary    ${params}    q=${q}
    IF    '${csf}'!=''    Set To Dictionary    ${params}    csf=${csf}
    IF    '${georel}'!=''    Set To Dictionary    ${params}    georel=${georel}
    IF    '${geometry}'!=''
        Set To Dictionary    ${params}    geometry=${geometry}
    END
    IF    '${coordinates}'!=''
        Set To Dictionary    ${params}    coordinates=${coordinates}
    END
    IF    '${geoproperty}'!=''
        Set To Dictionary    ${params}    geoproperty=${geoproperty}
    END
    IF    '${timeproperty}'!=''
        Set To Dictionary    ${params}    timeproperty=${timeproperty}
    END
    IF    '${timerel}'!=''
        Set To Dictionary    ${params}    timerel=${timerel}
    END
    IF    '${timeAt}'!=''    Set To Dictionary    ${params}    timeAt=${timeAt}
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    '${offset}'!=''    Set To Dictionary    ${params}    offset=${offset}

    ${response}=    GET
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Context Source Registrations
    RETURN    ${response}

Query Context Source Registrations With Return
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${context}=${EMPTY}
    ...    ${id}=${EMPTY}
    ...    ${type}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${q}=${EMPTY}
    ...    ${csf}=${EMPTY}
    ...    ${scopeQ}=${EMPTY}
    ...    ${georel}=${EMPTY}
    ...    ${geometry}=${EMPTY}
    ...    ${coordinates}=${EMPTY}
    ...    ${geoproperty}=${EMPTY}
    ...    ${timeproperty}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${limit}=${EMPTY}
    ...    ${offset}=${EMPTY}
    ...    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${id}'!=''    Set To Dictionary    ${params}    id=${id}
    IF    '${type}'!=''    Set To Dictionary    ${params}    type=${type}
    IF    '${attrs}'!=''    Set To Dictionary    ${params}    attrs=${attrs}
    IF    '${q}'!=''    Set To Dictionary    ${params}    q=${q}
    IF    '${csf}'!=''    Set To Dictionary    ${params}    csf=${csf}
    IF    '${scopeQ}'!=''    Set To Dictionary    ${params}    scopeQ=${scopeQ}
    IF    '${georel}'!=''    Set To Dictionary    ${params}    georel=${georel}
    IF    '${geometry}'!=''
        Set To Dictionary    ${params}    geometry=${geometry}
    END
    IF    '${coordinates}'!=''
        Set To Dictionary    ${params}    coordinates=${coordinates}
    END
    IF    '${geoproperty}'!=''
        Set To Dictionary    ${params}    geoproperty=${geoproperty}
    END
    IF    '${timeproperty}'!=''
        Set To Dictionary    ${params}    timeproperty=${timeproperty}
    END
    IF    '${timerel}'!=''
        Set To Dictionary    ${params}    timerel=${timerel}
    END
    IF    '${timeAt}'!=''    Set To Dictionary    ${params}    timeAt=${timeAt}
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    '${offset}'!=''    Set To Dictionary    ${params}    offset=${offset}

    ${response}=    GET
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Context Source Registrations
    RETURN    ${response}

Retrieve Context Source Registration
    [Tags]    actor_context-consumer
    [Arguments]    ${context_source_registration_id}    ${context}=${EMPTY}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    GET
    ...    url=${url}/${CONTEXT_SOURCE_REGISTRATION_ENDPOINT_PATH}/${context_source_registration_id}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Context Source Registration
    RETURN    ${response}
