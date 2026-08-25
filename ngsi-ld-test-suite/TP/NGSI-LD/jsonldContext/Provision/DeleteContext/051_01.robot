*** Settings ***
Documentation       Check that one can delete a previous created hosted @context without reload param

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial hosted @context


*** Variables ***
${filename}=        @context-minimal-valid.json
${reason_204}=      No Content


*** Test Cases ***
051_01_01 Delete A @context Whose Kind Is Hosted Without Reload Param
    [Documentation]    Check that one can delete a hosted @context
    [Tags]    ctx-serve    5_13_5    since_v1.5.1

    ${response}=    Delete a @context    ${uri}

    Check Response Status Code    204    ${response.status_code}
    Check Response Body Is Empty    ${response}
    Check Response Reason set to    ${response.reason}    ${reason_204}
    Check Response Does Not Contain Body    ${response}


*** Keywords ***
Create Initial hosted @context
    ${response}=    Add a new @context    ${filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${uri}
