*** Settings ***
Documentation       Check that one cannot retrieve the temporal evolution of an entity with an invalid id (invalid URI)

Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
020_06_01 Retrieve The Temporal Evolution Of An Entity With An Invalid Id
    [Documentation]    Check that one cannot retrieve the temporal evolution of an entity with an invalid id (invalid URI)
    [Tags]    te-retrieve    5_7_3
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=invalidUri
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
