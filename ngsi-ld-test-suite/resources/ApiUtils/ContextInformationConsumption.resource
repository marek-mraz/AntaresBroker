*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         OperatingSystem
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${ATTRIBUTES_ENDPOINT_PATH}                 attributes
${ENTITIES_ENDPOINT_PATH}                   entities/
${ENTITY_OPERATIONS_QUERY_ENDPOINT_PATH}    entityOperations/query
${ENTITIES_TYPES_ENDPOINT_PATH}             types

${CONTENT_TYPE_JSON}                        application/json
${CONTENT_TYPE_LD_JSON}                     application/ld+json
${CONTENT_TYPE_GEOJSON}                     application/geo+json

${ERROR_TYPE_BAD_REQUEST_DATA}              https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_INVALID_REQUEST}               https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}            https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                                 ${EMPTY}


*** Keywords ***
Query Entities
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${entity_ids}=${EMPTY}
    ...    ${entity_types}=${EMPTY}
    ...    ${accept}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${context}=${EMPTY}
    ...    ${geoproperty}=${EMPTY}
    ...    ${options}=${EMPTY}
    ...    ${limit}=${EMPTY}
    ...    ${entity_id_pattern}=${EMPTY}
    ...    ${scopeq}=${EMPTY}
    ...    ${georel}=${EMPTY}
    ...    ${coordinates}=${EMPTY}
    ...    ${geometry}=${EMPTY}
    ...    ${count}=${EMPTY}
    ...    ${q}=${EMPTY}
    ...    ${local}=${EMPTY}
    ...    ${datasetId}=${EMPTY}
    ...    ${join}=${EMPTY}
    ...    ${joinLevel}=${EMPTY}
    ...    ${pick}=${EMPTY}
    ...    ${omit}=${EMPTY}
    ...    ${orderBy}=${EMPTY}
    ...    ${broker_url}=${EMPTY}
    # Table 6.4.3.2-1 `geometryProperty` — selects the top-level geometry for
    # GeoJSON output. Distinct from `geoproperty`, which targets the geoquery.
    ...    ${geometryProperty}=${EMPTY}
    # 4.9 `expandValues` — attribute list whose query-term values are expanded
    # against the @context before evaluation (EXAMPLE 12).
    ...    ${expandValues}=${EMPTY}
    # 4.12 pagination offset — page start within the result set.
    ...    ${offset}=${EMPTY}
    ${final_url}=    Set Variable If    '${broker_url}'!=''    ${broker_url}    ${url}
    ${attrs_length}=    Get Length    ${attrs}
    ${accept_length}=    Get Length    ${accept}
    ${options_length}=    Get Length    ${options}
    ${entity_ids_length}=    Get Length    ${entity_ids}
    ${entity_types_length}=    Get Length    ${entity_types}
    ${pick_length}=    Get Length    ${pick}
    ${omit_length}=    Get Length    ${omit}
    ${orderBy_length}=    Get Length    ${orderBy}
    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    ${accept_length}>0
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    ${attrs_length}>0    Set To Dictionary    ${params}    attrs=${attrs}
    IF    '${geoproperty}'!=''
        Set To Dictionary    ${params}    geoproperty=${geoproperty}
    END
    IF    ${options_length}>0
        Set To Dictionary    ${params}    options=${options}
    END
    IF    ${entity_ids_length}>0
        Set To Dictionary    ${params}    id=${entity_ids}
    END
    IF    ${entity_types_length}>0
        Set To Dictionary    ${params}    type=${entity_types}
    END
    IF    '${limit}'!=''    Set To Dictionary    ${params}    limit=${limit}
    IF    '${entity_id_pattern}'!=''
        Set To Dictionary    ${params}    idPattern=${entity_id_pattern}
    END
    IF    '${scopeq}'!=''    Set To Dictionary    ${params}    scopeQ=${scopeq}
    IF    '${georel}'!=''    Set To Dictionary    ${params}    georel=${georel}
    IF    '${coordinates}'!=''
        Set To Dictionary    ${params}    coordinates=${coordinates}
    END
    IF    '${geometry}'!=''
        Set To Dictionary    ${params}    geometry=${geometry}
    END
    IF    '${count}'!=''    Set To Dictionary    ${params}    count=${count}
    IF    '${q}'!=''    Set To Dictionary    ${params}    q=${q}
    IF    '${local}'!=''    Set To Dictionary    ${params}    local=${local}
    IF    '${datasetId}'!=''
        Set To Dictionary    ${params}    datasetId=${datasetId}
    END
    IF    '${join}'!=''    Set To Dictionary    ${params}    join=${join}
    IF    '${join_level}'!=''
        Set To Dictionary    ${params}    joinLevel=${joinLevel}
    END
    IF    ${pick_length}>0    Set To Dictionary    ${params}    pick=${pick}
    IF    ${omit_length}>0    Set To Dictionary    ${params}    omit=${omit}
    IF    ${orderBy_length}>0
        Set To Dictionary    ${params}    orderBy=${orderBy}
    END
    IF    '${geometryProperty}'!=''
        Set To Dictionary    ${params}    geometryProperty=${geometryProperty}
    END
    IF    '${expandValues}'!=''
        Set To Dictionary    ${params}    expandValues=${expandValues}
    END
    IF    '${offset}'!=''    Set To Dictionary    ${params}    offset=${offset}

    ${response}=    GET
    ...    url=${final_url}/${ENTITIES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Entities
    RETURN    ${response}

Query Entities Via POST
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${entities}=${EMPTY}
    ...    ${content_type}=${CONTENT_TYPE_JSON}
    ...    ${accept}=${CONTENT_TYPE_JSON}
    ...    ${context}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${geometry_property}=${EMPTY}
    ...    ${join}=${EMPTY}
    ...    ${joinLevel}=${EMPTY}
    ...    ${local}=${EMPTY}
    ...    ${options}=${EMPTY}
    ...    ${broker_url}=${EMPTY}
    ${final_url}=    Set Variable If    '${broker_url}'!=''    ${broker_url}    ${url}
    &{headers}=    Create Dictionary
    Set To Dictionary    ${headers}    Content-Type    ${content_type}
    Set To Dictionary    ${headers}    Accept    ${accept}
    # NGSI-LD 6.3.5: for POST with Content-Type application/ld+json the @context MUST be carried in the
    # request body, and a JSON-LD Link header MUST NOT be present (its presence is itself BadRequestData).
    # For application/json the @context is carried in the Link header. Honour the content type here so
    # ld+json queries are not rejected with "@context entry missing".
    IF    '${context}'!='' and 'ld+json' not in '${content_type}'
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END

    &{params}=    Create Dictionary
    IF    '${options}'!=''
        Set To Dictionary    ${params}    options=${options}
    END
    IF    '${local}'!=''    Set To Dictionary    ${params}    local=${local}

    &{body}=    Create Dictionary
    Set To Dictionary    ${body}    type=Query
    ${entities_length}=    Get Length    ${entities}
    IF    ${entities_length}>0
        Set To Dictionary    ${body}    entities=${entities}
    END
    ${attrs_length}=    Get Length    ${attrs}
    IF    ${attrs_length}>0    Set To Dictionary    ${body}    attrs=${attrs}
    IF    '${geometry_property}'!=''
        Set To Dictionary    ${body}    geometryProperty=${geometry_property}
    END
    IF    '${join}'!=''    Set To Dictionary    ${body}    join=${join}
    IF    '${join_level}'!=''
        Set To Dictionary    ${body}    joinLevel=${joinLevel}
    END
    IF    '${context}'!='' and 'ld+json' in '${content_type}'
        Set To Dictionary    ${body}    @context=${context}
    END

    ${response}=    POST
    ...    url=${final_url}/${ENTITY_OPERATIONS_QUERY_ENDPOINT_PATH}
    ...    json=${body}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Query Entities Via POST
    RETURN    ${response}

Retrieve Entity
    [Tags]    actor_context-consumer
    [Arguments]
    ...    ${id}
    ...    ${accept}=${EMPTY}
    ...    ${attrs}=${EMPTY}
    ...    ${context}=${EMPTY}
    ...    ${geometryProperty}=${EMPTY}
    ...    ${options}=${EMPTY}
    ...    ${format}=${EMPTY}
    ...    ${lang}=${EMPTY}
    ...    ${join}=${EMPTY}
    ...    ${joinLevel}=${EMPTY}
    ...    ${pick}=${EMPTY}
    ...    ${omit}=${EMPTY}
    ...    ${local}=${EMPTY}
    ...    ${broker_url}=${EMPTY}

    ${attrs_length}=    Get Length    ${attrs}
    ${accept_length}=    Get Length    ${accept}
    ${options_length}=    Get Length    ${options}
    ${format_length}=    Get Length    ${format}
    ${lang_length}=    Get Length    ${lang}
    ${pick_length}=    Get Length    ${pick}
    ${omit_length}=    Get Length    ${omit}
    ${final_url}=    Set Variable If    '${broker_url}' != ''    ${broker_url}    ${url}
    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    ${accept_length}>0
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    IF    ${attrs_length}>0    Set To Dictionary    ${params}    attrs=${attrs}
    IF    '${geometryProperty}'!=''
        Set To Dictionary    ${params}    geometryProperty=${geometryProperty}
    END
    IF    ${options_length}>0
        Set To Dictionary    ${params}    options=${options}
    END
    IF    ${format_length}>0
        Set To Dictionary    ${params}    format=${format}
    END
    IF    ${lang_length}>0    Set To Dictionary    ${params}    lang=${lang}
    IF    '${join}'!=''    Set To Dictionary    ${params}    join=${join}
    IF    '${join_level}'!=''
        Set To Dictionary    ${params}    joinLevel=${joinLevel}
    END
    IF    ${pick_length}>0    Set To Dictionary    ${params}    pick=${pick}
    IF    ${omit_length}>0    Set To Dictionary    ${params}    omit=${omit}
    IF    '${local}'!=''    Set To Dictionary    ${params}    local=${local}

    ${response}=    GET
    ...    url=${final_url}/${ENTITIES_ENDPOINT_PATH}${id}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Retrieve Entity
    RETURN    ${response}

Retrieve Attribute
    [Tags]    actor_context-consumer
    [Arguments]    ${attribute_name}    ${context}=${EMPTY}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
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
    ...    url=${url}/${ATTRIBUTES_ENDPOINT_PATH}/${attribute_name}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Attribute
    RETURN    ${response}

Retrieve Attributes
    [Tags]    actor_context-consumer
    [Arguments]    ${context}=${EMPTY}    ${details}=false    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    Set To Dictionary    ${params}    details=${details}
    ${response}=    GET
    ...    url=${url}/${ATTRIBUTES_ENDPOINT_PATH}
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Retrieve Attributes
    RETURN    ${response}

Retrieve Entity Type
    [Tags]    actor_context-consumer
    [Arguments]    ${type}    ${context}=${EMPTY}    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
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
    ...    url=${url}/${ENTITIES_TYPES_ENDPOINT_PATH}/${type}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Retrieve Entity Type
    RETURN    ${response}

Retrieve Entity Types
    [Tags]    actor_context-consumer
    [Arguments]    ${context}=${EMPTY}    ${details}=false    ${accept}=${EMPTY}

    &{headers}=    Create Dictionary
    &{params}=    Create Dictionary
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    Set To Dictionary    ${params}    details=${details}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_TYPES_ENDPOINT_PATH}/
    ...    headers=${headers}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Retrieve Entity Types
    RETURN    ${response}
