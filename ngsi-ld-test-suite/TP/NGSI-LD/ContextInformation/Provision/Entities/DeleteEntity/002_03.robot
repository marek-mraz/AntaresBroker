*** Settings ***
Documentation       Check that one cannot delete an entity if the entity id is not known to the system

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Variables ***
${expected_status_code}=    404


*** Test Cases ***
002_03_01 Delete An Entity With An Id Not Known To The System
    [Documentation]    Check that one cannot delete an entity if the entity id is not known to the system
    [Tags]    e-delete    5_6_6
    ${entity_id}=    Generate Random Building Entity Id
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_RESOURCE_NOT_FOUND}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
