*** Settings ***
Documentation       Check that an error is raised if one deletes a temporal entity with a non-existing/invalid EntityId

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${status_code}=     404


*** Test Cases ***
009_03_01 Delete A Temporal Representation Of An Entity With An Unknown Entity Id
    [Documentation]    Check that an error is raised if one deletes a temporal entity with a non-existing entity id
    [Tags]    te-delete    5_6_16
    ${temporal_entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Delete Temporal Representation Of Entity With Returning Response    ${temporal_entity_id}
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
