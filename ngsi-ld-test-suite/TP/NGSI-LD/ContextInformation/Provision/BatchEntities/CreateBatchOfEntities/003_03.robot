*** Settings ***
Documentation       Check that one cannot create a batch of entities with an invalid request

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Create Batch Entity With Invalid Request Scenarios


*** Test Cases ***    FILENAME    PROBLEM_TYPE
003_03_01 InvalidJson
    [Tags]    be-create    5_6_7
    batch/invalid-json.jsonld    ${ERROR_TYPE_INVALID_REQUEST}
003_03_02 EmptyJson
    [Tags]    be-create    5_6_7
    batch/empty.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}


*** Keywords ***
Create Batch Entity With Invalid Request Scenarios
    [Documentation]    Check that one cannot create a batch of entities with an invalid request
    [Arguments]    ${filename}    ${problem_type}
    ${response}=    Batch Request Entities From File    create    filename=${filename}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${problem_type}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
