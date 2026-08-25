*** Settings ***
Documentation       Check that one can list all the @context available in the broker with several add @contexts with details equal to true

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
052_05_01 List @contexts With Details Set To True And No Kind And With Previously Several Add @contexts
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    true    ${EMPTY}
052_05_02 List @contexts With Details Set To True And Kind Set To Hosted And With Previously Several Add @contexts
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    true    Hosted
# move to new tests this doesn't work like this. cached need to run differently
# 052_05_03 List @contexts with details set to true and kind set to cached abd with previously several add @contexts
#    [Tags]    ctx-list    5_13_3    since_v1.5.1
#    true    Cached
# 052_05_04 List @contexts with details set to true and kind set to implicitlycreated and with previously several add @contexts
#    [Tags]    ctx-list    5_13_3    since_v1.5.1
#    true    ImplicitlyCreated


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

    @{uris}=    Create List
    Append To List    ${uris}    ${first_uri}
    Append To List    ${uris}    ${second_uri}
    Append To List    ${uris}    ${third_uri}
    Set Suite Variable    ${uris}

List @contexts with several previous created @context
    [Documentation]    Check that one can list @contexts
    [Arguments]    ${details}    ${kind}

    ${response}=    List @contexts    ${details}    ${kind}

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}

    # One needs to check the list of responses
    Check Context Response Body Containing a list of identifiers
    ...    ${response.json()}
    ...    ${uris}
    ...    ${kind}
    ...    ${TRUE}

Delete Initial @contexts
    FOR    ${uri}    IN    @{uris}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
