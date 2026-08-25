*** Settings ***
Documentation       Check that one can list all the @context available in the broker with one add @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial @context
Test Teardown       Delete Initial @context
Test Template       List @contexts with one previous created @context


*** Variables ***
${first_filename}=      @context-minimal-valid.json
${reason_200}=          OK
${reason_204}=          No Content


*** Test Cases ***    DETAILS    KIND
052_02_01 List @contexts With Neither Details Or Kind And With Previously One Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ${EMPTY}
052_02_02 List @contexts With No Details And Kind Equal To Hosted And With Previously One Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Hosted
052_02_03 List @contexts With Details Equal To False And No Kind And With Previously One Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ${EMPTY}
052_02_04 List @contexts With Details Equal To False And Kind Equal To Hosted And With Previously One Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Hosted


*** Keywords ***
Create Initial @context
    ${response}=    Add a new @context    ${first_filename}
    Check Response Status Code    201    ${response.status_code}
    ${uri}=    Fetch Id From Response Location Header    ${response.headers}
    @{uri_list}=    Create List
    Append To List    ${uri_list}    ${uri}
    Set Suite Variable    ${uri_list}

List @contexts with one previous created @context
    [Documentation]    Check that one can list @contexts
    [Arguments]    ${details}    ${kind}
    ${response}=    List @contexts    ${details}    ${kind}
    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Context Response Body Containing a list of identifiers
    ...    ${response.json()}
    ...    ${uri_list}
    ...    ${kind}
    ...    ${FALSE}

Delete Initial @context
    Delete a @context    ${uri_list[0]}
