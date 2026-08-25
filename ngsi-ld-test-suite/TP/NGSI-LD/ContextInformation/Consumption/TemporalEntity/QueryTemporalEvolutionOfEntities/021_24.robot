*** Settings ***
Documentation       Check that one can not query the temporal evolution of entities if it has none of the minimal parameters

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${entity_id_one}=       urn:ngsi-ld:Entity:01
${entity_id_two}=       urn:ngsi-ld:Entity:02


*** Test Cases ***
021_24 Query Temporal Evolution Of Entities Without Minimal Parameters
    [Documentation]    Check that one can not query the temporal evolution of entities if it has none of the minimal parameters
    [Tags]    te-query    5_7_4
    ${entities_ids_to_be_retrieved}=    Catenate    SEPARATOR=,    ${entity_id_one}    ${entity_id_two}

    ${response}=    Query Temporal Representation Of Entities
    ...    entity_ids=${entities_ids_to_be_retrieved}
    ...    timerel=after
    ...    timeAt=2020-07-01T12:05:00Z
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
