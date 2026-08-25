*** Settings ***
Documentation       Check that one cannot get an entity with invalid id

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Get Entity With Invalid Id


*** Test Cases ***    ENTITY_ID    EXPECTED_STATUS_CODE    PROBLEM_TYPE
018_02_01 Get An Entity If The Entity Id Is Not A Valid URI
    thisisaninvaliduri    400    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Get Entity With Invalid Id
    [Documentation]    Check that one cannot get an entity with invalid/missing id
    [Tags]    e-retrieve    5_7_1
    [Arguments]    ${entity_id}    ${expected_status_code}    ${problem_type}
    ${response}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${problem_type}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
