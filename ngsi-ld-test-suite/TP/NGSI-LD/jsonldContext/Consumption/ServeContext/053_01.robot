*** Settings ***
Documentation       Check that one can serve a previous created @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context
Test Teardown       Delete Initial @context
Test Template       Serve a @context with details


*** Variables ***
${filename}=        @context-minimal-valid.json
${reason_200}=      OK
${reason_204}=      No Content


*** Test Cases ***    DETAILS
053_01_01 Serve A @context Without Details
    [Tags]    ctx-serve    5_13_4    since_v1.5.1
    ${EMPTY}
053_01_02 Serve A @context With Details Equal To False
    [Tags]    ctx-serve    5_13_4    since_v1.5.1
    false


*** Keywords ***
Create Initial @context
    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}

Serve a @context with details
    [Documentation]    Check that one can serve a @context with details equal to empty or false
    [Arguments]    ${details}

    ${response}=    Serve a @context    ${uri}    ${details}

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Response Headers Containing Content-Type set to    ${CONTENT_TYPE_JSON}    ${response.headers}
    Check Context Response Body Content    ${filename}    ${response.json()}

Delete Initial @context
    Delete a @context    ${uri}
