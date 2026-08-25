*** Settings ***
Documentation       Check that one can list all the @context available in the broker with several add @contexts

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/HttpUtils.resource

Test Setup          Create Initial set of @contexts
Test Teardown       Delete Initial @contexts
Test Template       List @contexts with several previous created @context


*** Variables ***
${first_filename}=      @context-minimal-valid.json
${second_filename}=     @context-minimal-second-valid.json
${third_filename}=      @context-minimal-third-valid.json
${reason_200}=          OK
${reason_204}=          No Content


*** Test Cases ***    DETAILS    KIND
052_03_01 List @contexts With Neither Details Or Kind And With Previously Several Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ${EMPTY}
052_03_02 List @contexts With No Details And Kind Equal To Hosted And With Previously Several Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Hosted
052_03_03 List @contexts With Details Equal To False And No Kind And With Previously Several Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ${EMPTY}
052_03_04 List @contexts With Details Equal To False And Kind Equal To Hosted And With Previously Several Add @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Hosted


*** Keywords ***
Create Initial set of @contexts
    ${response}=    Add a new @context    ${first_filename}
    Check Response Status Code    201    ${response.status_code}
    ${first_uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${first_uri}

    ${response}=    Add a new @context    ${second_filename}
    Check Response Status Code    201    ${response.status_code}
    ${second_uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${second_uri}

    ${response}=    Add a new @context    ${third_filename}
    Check Response Status Code    201    ${response.status_code}
    ${third_uri}=    Fetch Id From Response Location Header    ${response.headers}
    Set Suite Variable    ${third_uri}

    @{uri_list}=    Create List
    Append To List    ${uri_list}    ${first_uri}
    Append To List    ${uri_list}    ${second_uri}
    Append To List    ${uri_list}    ${third_uri}
    Set Suite Variable    ${uri_list}

List @contexts with several previous created @context
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

Delete Initial @contexts
    FOR    ${uri}    IN    @{uri_list}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
