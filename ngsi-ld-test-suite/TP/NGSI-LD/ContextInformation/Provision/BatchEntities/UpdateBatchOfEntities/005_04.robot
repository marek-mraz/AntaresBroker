*** Settings ***
Documentation       Check that one cannot update a batch of entities with an invalid request

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       Batch Update Entity With Invalid Request Scenarios


*** Test Cases ***    FILENAME    PROBLEM_TYPE    EXPECTED_STATUS_CODE
005_04_01 InvalidJson
    [Tags]    be-update    5_6_9
    batch/invalid-json.jsonld    ${ERROR_TYPE_INVALID_REQUEST}    400
005_04_02 InvalidJsonLd
    [Tags]    be-update    5_6_9
    batch/invalid-json-ld.jsonld    ${ERROR_TYPE_BAD_REQUEST_DATA}    207


*** Keywords ***
Batch Update Entity With Invalid Request Scenarios
    [Documentation]    Check that one cannot update a batch of entities with an invalid request
    [Arguments]    ${filename}    ${problem_type}    ${expected_status_code}
    ${response}=    Batch Request Entities From File    update    filename=${filename}
    Check Response Status Code    ${expected_status_code}    ${response.status_code}
    IF    '${expected_status_code}'=='400'
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    response_body=${response.json()}
        ...    type=${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element
        ...    response_body=${response.json()}
    ELSE
        Check Response Body Containing ProblemDetails Element Containing Type Element set to
        ...    response_body=${response.json()['errors'][0]['error']}
        ...    type=${problem_type}
        Check Response Body Containing ProblemDetails Element Containing Title Element
        ...    response_body=${response.json()['errors'][0]['error']}
    END
