*** Settings ***
Documentation       Keywords for the EntityMap API (CIM 009 5.14; resources
...                 6.32 /entityMaps/{id}, 6.34 /entityMaps, 6.35
...                 /temporal/entityMaps). Antares extension — the official
...                 suite has no EntityMap coverage.

Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         RequestsLibrary


*** Variables ***
${ENTITYMAPS_ENDPOINT_PATH}     entityMaps

${ERROR_TYPE_BAD_REQUEST_DATA}      https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_RESOURCE_NOT_FOUND}    https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound


*** Keywords ***
Create EntityMap For Query
    [Documentation]    GET /entityMaps with query parameters (6.34.3.1).
    [Arguments]    &{params}
    ${response}=    GET
    ...    url=${url}/${ENTITYMAPS_ENDPOINT_PATH}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Create EntityMap For Query
    RETURN    ${response}

Create Temporal EntityMap For Query
    [Documentation]    GET /temporal/entityMaps with query parameters (6.35.3.1).
    [Arguments]    &{params}
    ${response}=    GET
    ...    url=${url}/temporal/${ENTITYMAPS_ENDPOINT_PATH}
    ...    params=${params}
    ...    expected_status=any
    Output    ${response}    Create Temporal EntityMap For Query
    RETURN    ${response}

Retrieve EntityMap
    [Documentation]    GET /entityMaps/{entityMapId} (6.32.3.1).
    [Arguments]    ${entityMapId}
    ${entityMapId}=    Evaluate    urllib.parse.quote("${entityMapId}", safe='')    modules=urllib
    ${response}=    GET
    ...    url=${url}/${ENTITYMAPS_ENDPOINT_PATH}/${entityMapId}
    ...    expected_status=any
    Output    ${response}    Retrieve EntityMap
    RETURN    ${response}

Update EntityMap
    [Documentation]    PATCH /entityMaps/{entityMapId} (6.32.3.2).
    [Arguments]    ${entityMapId}    ${fragment}
    ${entityMapId}=    Evaluate    urllib.parse.quote("${entityMapId}", safe='')    modules=urllib
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${response}=    PATCH
    ...    url=${url}/${ENTITYMAPS_ENDPOINT_PATH}/${entityMapId}
    ...    data=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Update EntityMap
    RETURN    ${response}

Delete EntityMap
    [Documentation]    DELETE /entityMaps/{entityMapId} (6.32.3.3).
    [Arguments]    ${entityMapId}
    ${entityMapId}=    Evaluate    urllib.parse.quote("${entityMapId}", safe='')    modules=urllib
    ${response}=    DELETE
    ...    url=${url}/${ENTITYMAPS_ENDPOINT_PATH}/${entityMapId}
    ...    expected_status=any
    Output    ${response}    Delete EntityMap
    RETURN    ${response}

Fetch EntityMap Id From Response
    [Documentation]    The NGSILD-EntityMap header carries the resource URI of
    ...    the EntityMap (6.34.3.1); its last segment is the map id.
    [Arguments]    ${headers}
    ${location}=    Get From Dictionary    ${headers}    NGSILD-EntityMap
    ${entityMapId}=    Evaluate    "${location}".rsplit("/", 1)[-1]
    RETURN    ${entityMapId}

Create EntityMap Test Entity
    [Documentation]    Minimal Vehicle entity used as EntityMap fixture.
    [Arguments]    ${entityId}    ${speed}
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${body}=    Evaluate
    ...    {"id": $entityId, "type": "Vehicle", "speed": {"type": "Property", "value": int($speed)}}
    ${response}=    POST
    ...    url=${url}/entities
    ...    json=${body}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create EntityMap Test Entity
    RETURN    ${response}

Delete EntityMap Test Entity
    [Arguments]    ${entityId}
    ${entityId}=    Evaluate    urllib.parse.quote("${entityId}", safe='')    modules=urllib
    ${response}=    DELETE
    ...    url=${url}/entities/${entityId}
    ...    expected_status=any
    RETURN    ${response}
