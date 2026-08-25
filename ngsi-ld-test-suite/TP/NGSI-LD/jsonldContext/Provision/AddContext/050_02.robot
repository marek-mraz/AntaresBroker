*** Settings ***
Documentation       Check that one receives a 400 Bad Request creating a @context if the content is incorrect

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Teardown       Delete @context if it was created by error
Test Template       Add @context scenarios


*** Variables ***
${wrong_context_filename}=      @context-incorrect.json
${wrong_json_filename}=         @context-wrong-json.json
${reason_400}=                  Bad Request
${problem_type}=                https://uri.etsi.org/ngsi-ld/errors/InvalidRequest


*** Test Cases ***    FILENAME    REASON
050_02_01 Checking Incorrect Payload
    [Tags]    ctx-add    5_13_2    since_v1.5.1
    ${wrong_context_filename}    ${reason_400}
050_02_02 Checking Wrong JSON
    [Tags]    ctx-add    5_13_2    since_v1.5.1
    ${wrong_json_filename}    ${reason_400}


*** Keywords ***
Add @context scenarios
    [Documentation]    Check that the payload include "@context"
    [Arguments]    ${filename}    ${reason}

    ${response}=    Add a new @context    ${filename}
    Set Suite Variable    ${response}

    Check Response Status Code    400    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason}

    Check Response Body Containing ProblemDetails Element    ${response.json()}    ${problem_type}

Delete @context if it was created by error
    IF    '${response.status_code}'!='400'
        ${uri}=    Fetch Id From Response Location Header    ${response.headers}
        Delete a @context    ${uri}
    END
