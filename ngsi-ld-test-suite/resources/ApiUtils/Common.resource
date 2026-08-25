*** Settings ***
Variables       ${EXECDIR}/resources/variables.py
Library         ${EXECDIR}/libraries/logUtils.py
Library         RequestsLibrary
Library         OperatingSystem
Library         Collections
Library         JSONLibrary
Library         String


*** Variables ***
${BUILDING_ID_PREFIX}                   urn:ngsi-ld:Building:
${BUS_ID_PREFIX}                        urn:ngsi-ld:Bus:
${CITY_ID_PREFIX}                       urn:ngsi-ld:City:
${COUNTRY_ID_PREFIX}                    urn:ngsi-ld:Country:
${PARKING_ID_PREFIX}                    urn:ngsi-ld:OffStreetParking:
${VEHICLE_ID_PREFIX}                    urn:ngsi-ld:Vehicle:

${SUBSCRIPTION_ID_PREFIX}=              urn:ngsi-ld:Subscription:

${CSR_ID_PREFIX}=                       urn:ngsi-ld:ContextSourceRegistration:

${TEMPORAL_ENTITIES_ENDPOINT_PATH}      temporal/entities
${ENTITIES_TYPES_ENDPOINT_PATH}         types

${ERROR_TYPE_INVALID_REQUEST}           https://uri.etsi.org/ngsi-ld/errors/InvalidRequest
${ERROR_TYPE_RESOURCE_NOT_FOUND}        https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound


*** Keywords ***
Build Context Link
    [Arguments]    ${context}
    RETURN    <${context}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"

Generate Random Id
    [Arguments]    ${id_prefix}

    ${random_id}=    Generate Random String    16    [NUMBERS]
    ${entity_id}=    Catenate    ${id_prefix}${random_id}
    RETURN    ${entity_id}

Generate Random Building Entity Id
    ${entity_id}=    Generate Random Id    ${BUILDING_ID_PREFIX}
    RETURN    ${entity_id}

Generate Bus Entity Id
    [Arguments]    ${id_suffix}

    ${entity_id}=    Catenate    ${BUS_ID_PREFIX}${id_suffix}
    RETURN    ${entity_id}

Generate Random Parking Entity Id
    ${entity_id}=    Generate Random Id    ${PARKING_ID_PREFIX}
    RETURN    ${entity_id}

Generate Random Vehicle Entity Id
    ${entity_id}=    Generate Random Id    ${VEHICLE_ID_PREFIX}
    RETURN    ${entity_id}

Generate Random Subscription Id
    ${entity_id}=    Generate Random Id    ${SUBSCRIPTION_ID_PREFIX}
    RETURN    ${entity_id}

Generate Random CSR Id
    ${entity_id}=    Generate Random Id    ${CSR_ID_PREFIX}
    RETURN    ${entity_id}

Call Api Endpoint With Invalid Parameter
    [Arguments]    ${method}    ${endpoint}    ${filename}=${EMPTY}    ${extraParam}=${EMPTY}
    IF    '${filename}' != "${EMPTY}"
        ${file_content}=    Get File    ${EXECDIR}/data/${filename}
    END

    &{headers}=    Create Dictionary    Content-Type=application/ld+json

    IF    '${extraParam}' != "${empty}"
        ${params}=    Catenate    ?invalidParams=invalidValue&    ${extraParam}
    ELSE
        ${params}=    Catenate    ?invalidParams=invalidValue
    END

    # No multi method request function in the robotframework-requests library
    IF    "${method}"=="GET"
        ${response}=    GET
        ...    url=${url}/${endpoint}${params}
        ...    headers=${headers}
        ...    expected_status=any
    ELSE IF    "${method}"=="POST"
        ${response}=    POST
        ...    url=${url}/${endpoint}${params}
        ...    json=${file_content}
        ...    headers=${headers}
        ...    expected_status=any
    ELSE IF    "${method}"=="PUT"
        ${response}=    PUT
        ...    url=${url}/${endpoint}${params}
        ...    json=${file_content}
        ...    headers=${headers}
        ...    expected_status=any
    ELSE IF    "${method}"=="DELETE"
        ${response}=    DELETE
        ...    url=${url}/${endpoint}${params}
        ...    headers=${headers}
        ...    expected_status=any
    END

    Output    ${response}    Call endpoint With Invalid Parameter
    RETURN    ${response}
