*** Settings ***
Documentation       Check that one cannot delete a batch of entities with an invalid request

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Batch Delete Entity With Invalid Request Scenarios


*** Test Cases ***    FILENAME    PROBLEM_TYPE
006_03_01 InvalidJson
    [Tags]    be-delete    5_6_10
    batch/invalid-json.jsonld    ${ERROR_TYPE_INVALID_REQUEST}
006_03_02 EmptyJson
    [Tags]    be-delete    5_6_10
    batch/empty.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Batch Delete Entity With Invalid Request Scenarios
    [Documentation]    Check that one cannot delete a batch of entities with an invalid request
    [Arguments]    ${filename}    ${problem_type}
    ${response}=    Batch Request Entities From File    delete    filename=${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${problem_type}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
