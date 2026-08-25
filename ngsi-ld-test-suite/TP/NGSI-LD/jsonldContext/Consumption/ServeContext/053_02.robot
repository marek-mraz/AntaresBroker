*** Settings ***
Documentation       Check that one can serve a previous created @context with details equal to True

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context
Test Teardown       Delete Initial @context
Test Template       Serve a @context with details equal to true


*** Variables ***
${filename}=        @context-minimal-valid.json
${reason_200}=      OK
${reason_204}=      No Content


*** Test Cases ***    DETAILS    CONTEXT_TYPE
053_02_01 Serve A @context With Details Set To True
    [Tags]    ctx-serve    5_13_4    since_v1.5.1
    true    Hosted


*** Keywords ***
Create Initial @context
    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}

Serve a @context with details equal to true
    [Documentation]    Check that one can serve a @context with details
    [Arguments]    ${details}    ${context_type}

    ${response}=    Serve a @context    ${uri}    ${details}

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Response Headers Containing Content-Type set to    ${CONTENT_TYPE_JSON}    ${response.headers}

    # Check mandatory keys in the response (URL, localId, kind, timestamp) and their possible values
    Check Context Response Body Containing Detailed Information    ${response.json()}    ${context_type}

    # Check optional keys in the response (lastUsage, numberOfHits, extraInfo) and their possible values
    Check Response Body Might Contain Optional Fields    ${response.json()}    lastUsage
    Check Response Body Might Contain Optional Fields    ${response.json()}    numberOfHits
    Check Response Body Might Contain Optional Fields    ${response.json()}    extraInfo

    # Check that there is no other keys
    Check Context Detailed Information Keys    ${response.json()}

Delete Initial @context
    Delete a @context    ${uri}
