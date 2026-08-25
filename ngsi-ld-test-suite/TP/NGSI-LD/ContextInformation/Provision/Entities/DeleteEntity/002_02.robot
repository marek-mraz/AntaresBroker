*** Settings ***
Documentation       Check that one cannot delete an entity with invalid id

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Template       Delete Entity Scenarios


*** Test Cases ***    ENTITY_ID    EXPECTED_STATUS_CODE    PROBLEM_TYPE
002_02_01 Delete An Entity If The Entity Id Is Not A Valid URI
    [Tags]    e-delete    5_6_6
    thisisaninvaliduri    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Delete Entity Scenarios
    [Documentation]    Check that one cannot delete an entity with invalid id
    [Arguments]    ${entity_id}    ${expected_status_code}    ${problem_type}
    ${response}=    Delete Entity    ${entity_id}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    "${problem_type}"!="${EMPTY}"
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    ${response.json()}
        ...    ${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    END
