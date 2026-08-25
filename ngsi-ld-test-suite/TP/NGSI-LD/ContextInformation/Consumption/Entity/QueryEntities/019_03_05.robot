*** Settings ***
Documentation       Check that one cannot query entities if it has none of the minimal parameters

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${entity_id_one}=       urn:ngsi-ld:Entity:01
${entity_id_two}=       urn:ngsi-ld:Entity:02


*** Test Cases ***
019_03_05 Query Entities Without Minimal Parameters
    [Documentation]    Check that one cannot query entities if it has none of the minimal parameters
    [Tags]    e-query    5_7_2
    ${entities_ids_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_id_one}    ${entity_id_two}

    ${response}=    Query Entities
    ...    entity_ids=${entities_ids_to_be_retrieved}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
