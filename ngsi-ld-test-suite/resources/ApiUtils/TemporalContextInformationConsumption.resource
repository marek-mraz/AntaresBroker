*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         JSONLibrary
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${TEMPORAL_ENTITIES_ENDPOINT_PATH}              temporal/entities
${TEMPORAL_ENTITY_OPERATIONS_ENDPOINT_PATH}     temporal/entityOperations

${CONTENT_TYPE_JSON}                            application/json

${ERROR_TYPE_BAD_REQUEST_DATA}                  https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_RESOURCE_NOT_FOUND}                https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                                     ${EMPTY}


*** Keywords ***
Query Temporal Representation Of Entities
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${context}=${EMPTY}
    ...    ${entity_types}=${EMPTY}
    ...    ${entity_ids}=${EMPTY}
    ...    ${entity_id_pattern}=${EMPTY}
    ...    ${ngsild_query}=${EMPTY}
    ...    ${csf}=${EMPTY}
    ...    ${georel}=${EMPTY}
    ...    ${geometry}=${EMPTY}
    ...    ${coordinates}=${EMPTY}
    ...    ${geoproperty}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${timeproperty}=${EMPTY}
    ...    ${endTimeAt}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${limit}=${EMPTY}
    ...    ${lastN}=${EMPTY}
    ...    ${accept}=${EMPTY}
    ...    ${options}=${EMPTY}
    ...    ${format}=${EMPTY}
    ...    ${aggrMethods}=${EMPTY}
    ...    ${aggrPeriodDuration}=${EMPTY}
    ...    ${datasetId}=${EMPTY}
    ...    ${pick}=${EMPTY}
    ...    ${omit}=${EMPTY}
    ...    ${orderBy}=${EMPTY}
    ...    ${local}=${EMPTY}
    ${entity_types_length}=    Get Length    ${entity_types}
    ${entity_ids_length}=    Get Length    ${entity_ids}
    ${attrs_length}=    Get Length    ${attrs}
    ${pick_length}=    Get Length    ${pick}
    ${omit_length}=    Get Length    ${omit}
    ${orderBy_length}=    Get Length    ${orderBy}
    ${timeproperty_length}=    Get Length    ${timeproperty}
    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    $accept!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    $context!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    ${entity_types_length}>0
        Set To Dictionary    ${params}    type=${entity_types}
    END
    IF    ${entity_ids_length}>0
        Set To Dictionary    ${params}    id=${entity_ids}
    END
    IF    $timerel!=''    Set To Dictionary    ${params}    timerel=${timerel}
    IF    $timeAt!=''    Set To Dictionary    ${params}    timeAt=${timeAt}
    IF    $endTimeAt!=''
        Set To Dictionary    ${params}    endTimeAt=${endTimeAt}
    END
    IF    ${attrs_length}>0    Set To Dictionary    ${params}    attrs=${attrs}
    IF    $lastN!=''    Set To Dictionary    ${params}    lastN=${lastN}
    IF    $entity_id_pattern!=''
        Set To Dictionary    ${params}    idPattern=${entity_id_pattern}
    END
    IF    $ngsild_query!=''
        Set To Dictionary    ${params}    q=${ngsild_query}
    END
    IF    $csf!=''    Set To Dictionary    ${params}    csf=${csf}
    IF    $georel!=''    Set To Dictionary    ${params}    georel=${georel}
    IF    $geometry!=''
        Set To Dictionary    ${params}    geometry=${geometry}
    END
    IF    $coordinates!=''
        Set To Dictionary    ${params}    coordinates=${coordinates}
    END
    IF    $geoproperty!=''
        Set To Dictionary    ${params}    geoproperty=${geoproperty}
    END
    IF    $limit!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    $options!=''    Set To Dictionary    ${params}    options=${options}
    IF    $format!=''    Set To Dictionary    ${params}    format=${format}
    IF    $aggrMethods!=''
        Set To Dictionary    ${params}    aggrMethods=${aggrMethods}
    END
    IF    $aggrPeriodDuration!=''
        Set To Dictionary    ${params}    aggrPeriodDuration=${aggrPeriodDuration}
    END
    IF    $datasetId!=''
        Set To Dictionary    ${params}    datasetId=${datasetId}
    END
    IF    ${pick_length}>0    Set To Dictionary    ${params}    pick=${pick}
    IF    ${omit_length}>0    Set To Dictionary    ${params}    omit=${omit}
    IF    ${orderBy_length}>0
        Set To Dictionary    ${params}    orderBy=${orderBy}
    END
    IF    ${timeproperty_length}>0
        Set To Dictionary    ${params}    timeproperty=${timeproperty}
    END
    IF    '${local}'!=''    Set To Dictionary    ${params}    local=${local}

    ${response}=    GET
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Temporal Representation Of Entities
    RETURN    ${response}

Query Temporal Representation Of Entities Via Post
    [Tags]    actor_context-consumer
    [Arguments]    ${query_file_name}    ${content_type}=${CONTENT_TYPE_JSON}    ${context}=${EMPTY}
    ${headers}=    Create Dictionary
    Set To Dictionary    ${headers}    Content-Type    ${content_type}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${query_payload}=    Load JSON From File    ${EXECDIR}/data/temporalEntities/${query_file_name}
    ${response}=    POST
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITY_OPERATIONS_ENDPOINT_PATH}/query
    ...    json=${query_payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Query Temporal Representation Of Entities Via Post
    RETURN    ${response}

Query Temporal Representation Of Entities With Return
    [Arguments]
    ...    ${context}=${EMPTY}
    ...    ${entity_types}=${EMPTY}
    ...    ${entity_ids}=${EMPTY}
    ...    ${entity_id_pattern}=${EMPTY}
    ...    ${ngsild_query}=${EMPTY}
    ...    ${csf}=${EMPTY}
    ...    ${georel}=${EMPTY}
    ...    ${geometry}=${EMPTY}
    ...    ${coordinates}=${EMPTY}
    ...    ${geoproperty}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${limit}=${EMPTY}
    ...    ${lastN}=${EMPTY}
    ...    ${accept}=${EMPTY}
    ${entity_types_length}=    Get Length    ${entity_types}
    ${entity_ids_length}=    Get Length    ${entity_ids}
    ${attrs_length}=    Get Length    ${attrs}
    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    ${entity_types_length}>0
        Set To Dictionary    ${params}    type=${entity_types}
    END
    IF    ${entity_ids_length}>0
        Set To Dictionary    ${params}    id=${entity_ids}
    END
    IF    '${timerel}'!=''
        Set To Dictionary    ${params}    timerel=${timerel}
    END
    IF    '${timeAt}'!=''    Set To Dictionary    ${params}    timeAt=${timeAt}
    IF    ${attrs_length}>0    Set To Dictionary    ${params}    attrs=${attrs}
    IF    '${lastN}'!=''    Set To Dictionary    ${params}    lastN=${lastN}
    IF    '${entity_id_pattern}'!=''
        Set To Dictionary    ${params}    idPattern=${entity_id_pattern}
    END
    IF    '${ngsild_query}'!=''
        Set To Dictionary    ${params}    q=${ngsild_query}
    END
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
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}

    ${response}=    GET
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Temporal Representation Of Entities
    RETURN    ${response}

Retrieve Temporal Representation Of Entity
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${temporal_entity_representation_id}
    ...    ${attrs}=${EMPTY}
    ...    ${options}=${EMPTY}
    ...    ${context}=${EMPTY}
    ...    ${timerel}=${EMPTY}
    ...    ${timeAt}=${EMPTY}
    ...    ${endTimeAt}=${EMPTY}
    ...    ${lastN}=${EMPTY}
    ...    ${accept}=${EMPTY}
    ...    ${aggrMethods}=${EMPTY}
    ...    ${aggrPeriodDuration}=${EMPTY}
    ...    ${timeproperty}=${EMPTY}
    ...    ${pick}=${EMPTY}
    ...    ${omit}=${EMPTY}
    ${attrs_length}=    Get Length    ${attrs}
    ${options_length}=    Get Length    ${options}
    ${pick_length}=    Get Length    ${pick}
    ${omit_length}=    Get Length    ${omit}
    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary

    IF    '${accept}'!=''    Set To Dictionary    ${headers}    Accept=${accept}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    ${attrs_length}>0    Set To Dictionary    ${params}    attrs=${attrs}
    IF    ${options_length}>0
        Set To Dictionary    ${params}    options=${options}
    END
    IF    '${timerel}'!=''
        Set To Dictionary    ${params}    timerel=${timerel}
    END
    IF    '${timeAt}'!=''    Set To Dictionary    ${params}    timeAt=${timeAt}
    IF    '${endTimeAt}'!=''
        Set To Dictionary    ${params}    endTimeAt=${endTimeAt}
    END
    IF    '${lastN}'!=''    Set To Dictionary    ${params}    lastN=${lastN}
    IF    '${aggrMethods}'!=''
        Set To Dictionary    ${params}    aggrMethods=${aggrMethods}
    END
    IF    '${aggrPeriodDuration}'!=''
        Set To Dictionary    ${params}    aggrPeriodDuration=${aggrPeriodDuration}
    END
    IF    '${timeproperty}'!=''
        Set To Dictionary    ${params}    timeproperty=${timeproperty}
    END
    IF    ${pick_length}>0    Set To Dictionary    ${params}    pick=${pick}
    IF    ${omit_length}>0    Set To Dictionary    ${params}    omit=${omit}

    ${response}=    GET
    ...    url=${temporal_api_url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_representation_id}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Retrieve Temporal Representation Of Entity
    RETURN    ${response}
