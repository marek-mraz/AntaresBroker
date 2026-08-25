*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         Collections
Library         JSONLibrary
Library         OperatingSystem
Library         RequestsLibrary
Resource        ${EXECDIR}/resources/ApiUtils/Common.resource


*** Variables ***
${TEMPORAL_ENTITIES_ENDPOINT_PATH}      temporal/entities

${CONTENT_TYPE_JSON}                    application/json
${CONTENT_TYPE_LD_JSON}                 application/ld+json

${ERROR_TYPE_BAD_REQUEST_DATA}          https://uri.etsi.org/ngsi-ld/errors/BadRequestData
${ERROR_TYPE_INVALID_REQUEST}           https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}        https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound

${response}                             ${EMPTY}


*** Keywords ***
Append Attribute To Temporal Entity
    [Tags]    actor_context-producer
    [Arguments]    ${temporal_entity_id}    ${fragment_filename}    ${content_type}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    ${file_content}=    Get File    ${EXECDIR}/data/temporalEntities/fragments/${fragment_filename}
    ${response}=    POST
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_id}/attrs
    ...    data=${file_content}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Append Attribute To Temporal Entity
    RETURN    ${response}

Create Or Update Temporal Representation Of Entity Selecting Content Type
    [Tags]    actor_context-producer
    [Arguments]    ${temporal_entity_representation_id}    ${filename}    ${content_type}    ${accept}=${EMPTY}
    ${temporal_entity_representation_payload}=    Load JSON From File    ${EXECDIR}/data/temporalEntities/${filename}
    ${temporal_entity_representation}=    Update Value To JSON
    ...    ${temporal_entity_representation_payload}
    ...    $..id
    ...    ${temporal_entity_representation_id}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${accept}'!=''
        Set To Dictionary    ${headers}    Accept    ${accept}
    END
    ${response}=    POST
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/
    ...    json=${temporal_entity_representation}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Or Update Temporal Representation Of Entity Selecting Content Type
    RETURN    ${response}

Create Temporal Representation Of Entity
    [Tags]    actor_context-producer
    [Arguments]    ${filename}    ${temporal_entity_representation_id}
    ${temporal_entity_representation_payload}=    Load JSON From File    ${EXECDIR}/data/temporalEntities/${filename}
    ${temporal_entity_representation}=    Update Value To JSON
    ...    ${temporal_entity_representation_payload}
    ...    $..id
    ...    ${temporal_entity_representation_id}

    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    POST
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}
    ...    json=${temporal_entity_representation}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Temporal Representation Of Entity
    RETURN    ${response}

Create Temporal Representation Of Entity Selecting Content Type
    [Tags]    actor_context-producer
    [Arguments]    ${filename}    ${content_type}
    ${file_content}=    Get File    ${EXECDIR}/data/temporalEntities/${filename}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    ${response}=    POST
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}
    ...    data=${file_content}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Create Temporal Representation Of Entity Selecting Content Type
    RETURN    ${response}

Delete Attribute From Temporal Entity
    [Tags]    actor_context-producer
    [Arguments]
    ...    ${entityId}
    ...    ${attributeId}
    ...    ${content_type}
    ...    ${datasetId}
    ...    ${deleteAll}
    ...    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    @{params}=    Create List
    IF    '${datasetId}'!=''
        Append To List    ${params}    datasetId=${datasetId}
    END
    IF    '${deleteAll}'!=''
        Append To List    ${params}    deleteAll=${deleteAll}
    END
    ${params_as_string}=    Catenate    SEPARATOR=&    @{params}
    ${response}=    DELETE
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${entityId}/attrs/${attributeId}?${params_as_string}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Delete Attribute From Temporal Entity
    RETURN    ${response}

Delete Attribute Instance From Temporal Entity
    [Tags]    actor_context-producer
    [Arguments]    ${temporal_entity_id}    ${attributeId}    ${instanceId}    ${content_type}    ${context}=${EMPTY}
    &{headers}=    Create Dictionary
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${response}=    DELETE
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_id}/attrs/${attributeId}/${instanceId}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Delete Attribute Instance From Temporal Entity
    RETURN    ${response}

Delete Temporal Representation Of Entity
    [Tags]    actor_context-producer
    [Arguments]    ${temporal_entity_representation_id}

    ${response}=    DELETE
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_representation_id}
    ...    expected_status=any
    Output    ${response}    Delete Temporal Representation Of Entity
    RETURN    ${response}

Delete Temporal Representation Of Entity With Returning Response
    [Tags]    actor_context-producer
    [Arguments]    ${temporal_entity_representation_id}
    ${response}=    DELETE
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_representation_id}
    ...    expected_status=any
    Output    ${response}    Delete Temporal Representation Of Entity
    RETURN    ${response}

Modify Attribute Instance From Temporal Entity
    [Tags]    actor_context-producer
    [Arguments]
    ...    ${temporal_entity_id}
    ...    ${attributeId}
    ...    ${instanceId}
    ...    ${fragment_filename}
    ...    ${content_type}
    ...    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${context}'!=''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary
        ...    ${headers}
        ...    Link=${context_link}
    END
    ${fragment_payload}=    Load JSON From File    ${EXECDIR}/data/temporalEntities/fragments/${fragment_filename}
    ${response}=    PATCH
    ...    url=${url}/${TEMPORAL_ENTITIES_ENDPOINT_PATH}/${temporal_entity_id}/attrs/${attributeId}/${instanceId}
    ...    json=${fragment_payload}
    ...    headers=${headers}
    ...    expected_status=any
    Output    ${response}    Modify Attribute Instance From Temporal Entity
    RETURN    ${response}
